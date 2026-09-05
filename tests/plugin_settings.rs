//! End-to-end tests for the settings core plugin (the first real core plugin,
//! #115): the plugin is spawned over the real stdio wire with a stateful
//! in-memory [`KvStore`] (hand-rolled — the mockall `MockKvStore` is
//! per-call stateless, but the settings flow needs persistence across two
//! spawned instances). Pure stdio — no database, no Discord.
//!
//! Assertions mirror the plugin's documented contract:
//! - an `invoke` of `settings` answers the full envelope
//!   `{"data", "ephemeral", "view"}` whose data is a Components V2 container
//!   mirroring the original monolith hub, carrying the loaded (or default)
//!   model and the session's page in `view`;
//! - the first invoke loads the model from `host.kv.get` (namespace
//!   `settings`) and re-registers it via `host.kv.set` before rendering;
//! - `view.interact` on the toggle select (`settings:toggle`) flips every
//!   selected feature of the session's own model and persists it via
//!   `host.kv.set` before answering;
//! - an About click issues `host.stats` and renders the About panel with the
//!   live values (the fallback copy when the op fails); Back returns to the
//!   hub — neither touches the model. The page rides the per-session `view`
//!   payload, so concurrent hubs stay independent and every fresh invoke
//!   opens the hub;
//! - a `settings:config:<feature>` button re-renders the current page (the
//!   panels it would open are not plugins yet);
//! - a `settings:open:<plugin>` nav click issues `host.open_view`, opening
//!   the target plugin's panel through the manager and interaction engine;
//! - a second spawn sharing the same store renders the persisted model;
//! - `bye` exits cleanly with status 0.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use async_trait::async_trait;
use poise::serenity_prelude as serenity;
use pwr_bot::plugin::HostConfig;
use pwr_bot::plugin::HostIo;
use pwr_bot::plugin::HostServices;
use pwr_bot::plugin::InteractionEngine;
use pwr_bot::plugin::KvError;
use pwr_bot::plugin::KvStore;
use pwr_bot::plugin::PluginManager;
use pwr_bot::plugin::RespawnPolicy;
use pwr_bot::plugin::RunningPlugin;
use pwr_bot::plugin::StatsHandle;
use pwr_bot::plugin::StatsSource;
use pwr_bot::plugin::host::MockHostIo;
use pwr_bot::plugin::host::MockStatsSource;
use pwr_plugin_protocol::HostStats;
use pwr_plugin_protocol::Msg;
use pwr_poise_components::IS_COMPONENTS_V2;
use serde_json::Value;
use serde_json::json;

/// The KV namespace and model key the settings plugin persists under.
const KV_NAMESPACE: &str = "settings";
const KV_MODEL_KEY: &str = "model";

mod probe;
use probe::probe_binary;

/// A stateful in-memory [`KvStore`]: a `(namespace, key)` → value map shared
/// across every handle that holds it, so a second spawned instance observes
/// what the first persisted.
#[derive(Default)]
struct SharedKv {
    inner: Mutex<HashMap<(String, String), String>>,
}

impl SharedKv {
    fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// The persisted value for the settings model, if any.
    fn model_value(&self) -> Option<String> {
        self.inner
            .lock()
            .expect("kv lock")
            .get(&(KV_NAMESPACE.to_string(), KV_MODEL_KEY.to_string()))
            .cloned()
    }
}

#[async_trait]
impl KvStore for SharedKv {
    async fn get(&self, namespace: &str, key: &str) -> Result<Option<String>, KvError> {
        Ok(self
            .inner
            .lock()
            .expect("kv lock")
            .get(&(namespace.to_string(), key.to_string()))
            .cloned())
    }

    async fn set(&self, namespace: &str, key: &str, value: &str) -> Result<(), KvError> {
        self.inner
            .lock()
            .expect("kv lock")
            .insert((namespace.to_string(), key.to_string()), value.to_string());
        Ok(())
    }

    async fn delete(&self, namespace: &str, key: &str) -> Result<(), KvError> {
        self.inner
            .lock()
            .expect("kv lock")
            .remove(&(namespace.to_string(), key.to_string()));
        Ok(())
    }
}

fn host_services(kv: Option<Arc<dyn KvStore>>) -> Arc<HostServices> {
    Arc::new(HostServices {
        io: None,
        config: Some(HostConfig {
            db_url: "postgres://test".into(),
            data_path: PathBuf::from("/tmp/pwr-bot-test"),
            poll_interval: std::time::Duration::from_secs(30),
        }),
        kv,
        engine: None,
        stats: Arc::new(StatsHandle::default()),
    })
}

/// Like [`host_services`], but with the io seam and the interaction engine
/// wired in so `host.open_view` can post its placeholder and open the target
/// plugin's session while the settings model still loads from KV.
fn view_host_services(
    io: Arc<dyn HostIo>,
    kv: Arc<dyn KvStore>,
    engine: InteractionEngine<RunningPlugin>,
) -> Arc<HostServices> {
    Arc::new(HostServices {
        io: Some(io),
        config: Some(HostConfig {
            db_url: "postgres://test".into(),
            data_path: PathBuf::from("/tmp/pwr-bot-test"),
            poll_interval: std::time::Duration::from_secs(30),
        }),
        kv: Some(kv),
        engine: Some(Arc::new(engine)),
        stats: Arc::new(StatsHandle::default()),
    })
}

/// Like [`view_host_services`], but with a live `host.stats` source serving
/// the given snapshot, so the About panel renders real values.
fn stats_host_services(
    io: Arc<dyn HostIo>,
    kv: Arc<dyn KvStore>,
    engine: InteractionEngine<RunningPlugin>,
    stats: Arc<dyn StatsSource>,
) -> Arc<HostServices> {
    let handle = StatsHandle::default();
    handle.attach(stats);
    Arc::new(HostServices {
        io: Some(io),
        config: Some(HostConfig {
            db_url: "postgres://test".into(),
            data_path: PathBuf::from("/tmp/pwr-bot-test"),
            poll_interval: std::time::Duration::from_secs(30),
        }),
        kv: Some(kv),
        engine: Some(Arc::new(engine)),
        stats: Arc::new(handle),
    })
}

/// Spawns the settings plugin with the given KV store (or none).
async fn spawn_settings(kv: Option<Arc<dyn KvStore>>) -> RunningPlugin {
    RunningPlugin::spawn_with(
        probe_binary("settings"),
        Some(host_services(kv)),
        None,
        None,
    )
    .await
    .expect("spawn settings plugin")
}

/// Asserts a resp is the settings envelope and returns its `view` object.
/// `expected_id` is the host's call id this resp answers: each sequential
/// `plugin.call` on the same instance bumps the id (0, 1, 2, ...), and the
/// plugin echoes it back end to end. The hub is a Components V2 message, so
/// the payload carries the v2 flag and no legacy top-level content.
fn assert_envelope(resp: &Msg, expected_id: u64) -> Value {
    match resp {
        Msg::Resp {
            id,
            ok: true,
            data: Some(data),
            error: None,
        } => {
            assert_eq!(*id, expected_id, "resp answers its own invoke id");
            assert_eq!(data["ephemeral"], json!(false));
            assert_eq!(data["data"]["flags"], json!(IS_COMPONENTS_V2));
            assert!(
                data["data"].get("content").is_none(),
                "v2 payloads carry no top-level content"
            );
            data["view"].clone()
        }
        other => panic!("expected ok settings envelope, got {other:?}"),
    }
}

/// Asserts the envelope's `view` state carries the session model with the
/// given enabled state per feature.
fn assert_toggles(view: &Value, feeds: bool, voice: bool, welcome: bool) {
    let model = view["model"].as_object().expect("view carries a model");
    assert_eq!(model["feeds"], json!(feeds));
    assert_eq!(model["voice"], json!(voice));
    assert_eq!(model["welcome"], json!(welcome));
}

/// Asserts the envelope renders the original monolith hub layout as
/// Components V2: one container holding the header, both info sections,
/// the feature-button row, the toggle select (labels reflecting `enabled`),
/// and the plugin nav row; plus the About button row outside the container.
fn assert_hub(data: &Value, enabled: &[bool; 3]) {
    let components = data["data"]["components"].as_array().expect("components");
    assert_eq!(components.len(), 2, "container plus the About row");
    let about_row = &components[1];
    assert_eq!(about_row["type"], json!(1));
    assert_eq!(
        about_row["components"][0]["custom_id"],
        json!("settings:about")
    );

    let boxed = &components[0];
    assert_eq!(boxed["type"], json!(17));
    let children = boxed["components"].as_array().expect("container children");
    assert_eq!(
        children.len(),
        6,
        "header, configure info, buttons, toggle info, select, nav"
    );

    let header = &children[0];
    assert_eq!(header["type"], json!(10));
    assert_eq!(header["content"], json!("-# **Settings**"));

    let config_buttons = children[2]["components"].as_array().expect("config row");
    let config_ids = [
        "settings:config:feeds",
        "settings:config:voice",
        "settings:config:welcome",
    ];
    for (button, custom_id) in config_buttons.iter().zip(config_ids) {
        assert_eq!(button["type"], json!(2));
        assert_eq!(button["custom_id"], json!(custom_id));
        assert_eq!(button["style"], json!(2), "secondary like the original");
    }

    let select = &children[4]["components"][0];
    assert_eq!(select["type"], json!(3));
    assert_eq!(select["custom_id"], json!("settings:toggle"));
    let emojis = ["✅", "⬜"];
    let labels = ["Feeds", "Voice", "Welcome"];
    for (index, option) in select["options"]
        .as_array()
        .expect("options")
        .iter()
        .enumerate()
    {
        assert_eq!(
            option["label"],
            json!(format!(
                "{} {}",
                emojis[usize::from(!enabled[index])],
                labels[index]
            ))
        );
        assert_eq!(option["value"], json!(labels[index]));
    }

    let nav = &children[5];
    assert_eq!(nav["type"], json!(1));
    let nav_buttons = nav["components"].as_array().expect("nav components");
    assert_eq!(nav_buttons.len(), 1, "one nav button");
    assert_eq!(nav_buttons[0]["type"], json!(2));
    assert_eq!(nav_buttons[0]["custom_id"], json!("settings:open:hello"));
}

/// An `invoke` of `settings` answers the full envelope with the default model
/// (every feature disabled) after loading from and re-registering in KV.
#[tokio::test]
async fn invoke_answers_the_default_envelope_and_registers_in_kv() {
    let kv = SharedKv::new();
    let plugin = spawn_settings(Some(kv.clone())).await;

    let resp = plugin
        .call("invoke", Some("settings"), Some(json!({})))
        .await
        .expect("invoke answered");

    let view = assert_envelope(&resp, 0);
    assert_toggles(&view, false, false, false);
    // The first invoke loaded from KV (empty → default) and re-registered the
    // model before rendering (AC3 registration contract).
    assert_eq!(
        kv.model_value(),
        Some(
            json!({
                "feeds": false,
                "voice": false,
                "welcome": false
            })
            .to_string()
        ),
        "model registered in KV"
    );

    let status = plugin.stop().await.expect("graceful stop");
    assert_eq!(status.code(), Some(0), "clean exit after bye: {status}");
}

/// An `invoke` while the model is already loaded answers immediately with the
/// current model — no further KV traffic is needed.
#[tokio::test]
async fn second_invoke_answers_immediately_with_the_loaded_model() {
    let kv = SharedKv::new();
    let plugin = spawn_settings(Some(kv.clone())).await;

    plugin
        .call("invoke", Some("settings"), Some(json!({})))
        .await
        .expect("first invoke answered");

    let resp = plugin
        .call("invoke", Some("settings"), Some(json!({})))
        .await
        .expect("second invoke answered");

    let view = assert_envelope(&resp, 1);
    assert_toggles(&view, false, false, false);

    let status = plugin.stop().await.expect("graceful stop");
    assert_eq!(status.code(), Some(0), "clean exit after bye: {status}");
}

/// A `view.interact` on the toggle select flips the selected feature,
/// persists the model through `host.kv.set` (namespace `settings`), and
/// answers the fresh envelope.
#[tokio::test]
async fn interact_toggles_a_feature_and_persists_it() {
    let kv = SharedKv::new();
    let plugin = spawn_settings(Some(kv.clone())).await;

    plugin
        .call("invoke", Some("settings"), Some(json!({})))
        .await
        .expect("invoke answered");

    let resp = plugin
        .call(
            "view.interact",
            Some("settings"),
            Some(json!({
                "custom_id": "settings:toggle",
                "data": { "values": ["Feeds"] }
            })),
        )
        .await
        .expect("interact answered");

    let view = assert_envelope(&resp, 1);
    assert_toggles(&view, true, false, false);
    assert_eq!(
        kv.model_value(),
        Some(
            json!({
                "feeds": true,
                "voice": false,
                "welcome": false
            })
            .to_string()
        ),
        "toggled model persisted in KV"
    );

    let status = plugin.stop().await.expect("graceful stop");
    assert_eq!(status.code(), Some(0), "clean exit after bye: {status}");
}

/// A second spawned instance sharing the same KV store renders the persisted
/// model: state survives a restart (AC6 persistence proof).
#[tokio::test]
async fn a_restarted_plugin_renders_the_persisted_model() {
    let kv = SharedKv::new();

    // First instance: toggle feeds on, then stop.
    let first = spawn_settings(Some(kv.clone())).await;
    first
        .call("invoke", Some("settings"), Some(json!({})))
        .await
        .expect("first invoke answered");
    first
        .call(
            "view.interact",
            Some("settings"),
            Some(json!({
                "custom_id": "settings:toggle",
                "data": { "values": ["Feeds"] }
            })),
        )
        .await
        .expect("first toggle answered");
    let status = first.stop().await.expect("first stop");
    assert_eq!(status.code(), Some(0), "first instance exits 0: {status}");

    // Second instance, same store: the first invoke must load feeds=true.
    let second = spawn_settings(Some(kv.clone())).await;
    let resp = second
        .call("invoke", Some("settings"), Some(json!({})))
        .await
        .expect("second invoke answered");
    let view = assert_envelope(&resp, 0);
    assert_toggles(&view, true, false, false);
    let status = second.stop().await.expect("second stop");
    assert_eq!(status.code(), Some(0), "second instance exits 0: {status}");
}

/// The envelope renders the original hub layout: the v2 container with both
/// info sections, the feature buttons, and the toggle select whose labels
/// mirror the model.
#[tokio::test]
async fn envelope_renders_the_original_hub_layout() {
    let plugin = spawn_settings(None).await;

    let resp = plugin
        .call("invoke", Some("settings"), Some(json!({})))
        .await
        .expect("invoke answered");

    assert_envelope(&resp, 0);
    let Msg::Resp {
        data: Some(data), ..
    } = &resp
    else {
        panic!("expected ok envelope, got {resp:?}");
    };
    assert_hub(data, &[false, false, false]);

    let status = plugin.stop().await.expect("graceful stop");
    assert_eq!(status.code(), Some(0), "clean exit after bye: {status}");
}

/// Without a KV store the plugin still answers: the load failure falls back to
/// the default model and the render is not blocked (the failed save is
/// logged, not fatal).
#[tokio::test]
async fn invoke_answers_with_defaults_when_kv_is_unavailable() {
    let plugin = spawn_settings(None).await;

    let resp = plugin
        .call("invoke", Some("settings"), Some(json!({})))
        .await
        .expect("invoke answered");

    let view = assert_envelope(&resp, 0);
    assert_toggles(&view, false, false, false);

    let status = plugin.stop().await.expect("graceful stop");
    assert_eq!(status.code(), Some(0), "clean exit after bye: {status}");
}

/// The `settings:open:<plugin>` nav click issues `host.open_view` end to end:
/// the host resolves the target through the manager, posts a placeholder
/// through the io seam, opens the target's panel as an engine session on the
/// produced message id, and the settings plugin answers the interaction with
/// its own envelope. A subsequent interaction on the produced message routes
/// into the target plugin.
#[tokio::test]
async fn nav_click_opens_the_target_plugin_panel() {
    let channel_id = 987_654_321_u64;
    let produced = 123_456_789_u64;
    let mut mock = MockHostIo::new();
    mock.expect_send_message()
        .with(
            mockall::predicate::eq(channel_id),
            mockall::predicate::eq("Loading…"),
        )
        .times(1)
        .returning(move |_, _| Ok(Some(json!({ "message_id": produced }))));
    mock.expect_edit_message()
        .with(
            mockall::predicate::eq(channel_id),
            mockall::predicate::eq(produced),
            mockall::predicate::function(|data: &serde_json::Value| {
                // The arg-echo fixture renders a Components V2 payload: the
                // echoed args are the text of a text display. The edit
                // transport strips the create-only fields the fixture's
                // envelope carries (`sticker_ids`, `tts`, `enforce_nonce`):
                // Discord rejects them on edit.
                data == &json!({
                    "attachments": [],
                    "components": [{"content": "{}", "type": 10}],
                    "embeds": [],
                    "flags": 32768,
                })
            }),
        )
        .times(1)
        .returning(move |_, _, _| Ok(Some(json!({ "message_id": produced }))));

    let engine = InteractionEngine::new();
    let kv = SharedKv::new();
    let services = view_host_services(Arc::new(mock), kv.clone(), engine.clone());
    let manager = Arc::new(PluginManager::new(None, RespawnPolicy::default()));
    manager
        .spawn("arg-echo", probe_binary("arg_echo_plugin"), None, &[], &[])
        .await
        .expect("spawn target plugin");

    let settings = RunningPlugin::spawn_with(
        probe_binary("settings"),
        Some(services),
        Some(manager.clone()),
        None,
    )
    .await
    .expect("spawn settings plugin");
    settings
        .call("invoke", Some("settings"), Some(json!({})))
        .await
        .expect("invoke answered");

    let resp = settings
        .call(
            "view.interact",
            Some("settings"),
            Some(json!({
                "custom_id": "settings:open:arg-echo",
                "channel_id": channel_id,
            })),
        )
        .await
        .expect("nav click answered");

    let view = assert_envelope(&resp, 1);
    assert_toggles(&view, false, false, false);
    let status = settings.stop().await.expect("graceful stop");
    assert_eq!(status.code(), Some(0), "clean exit after bye: {status}");

    let message_id = serenity::MessageId::new(produced);
    assert!(
        engine.has_session(message_id).await,
        "the produced message has an open session"
    );
    let follow_up = engine
        .interact_validated(message_id, "arg-echo", json!({}), |data| {
            pwr_bot::plugin::validate_view_data(data).map_err(Into::into)
        })
        .await
        .expect("follow-up interaction routes to the target plugin");
    assert_eq!(
        follow_up.data["components"][0]["content"],
        "{\"custom_id\":\"arg-echo\",\"view\":{\"last_args\":{}}}"
    );
    assert_eq!(follow_up.data["tts"], false);
    assert_eq!(follow_up.data["enforce_nonce"], false);
    manager
        .unload("arg-echo", &[])
        .await
        .expect("stop target plugin");
}

/// The About click answers with the plugin-side About panel (a v2 container
/// holding a section with a link-button accessory) after one `host.stats`
/// round trip — which fails on a spawn without a stats source, so the
/// fallback copy shows — and Back restores the hub.
#[tokio::test]
async fn about_click_renders_the_about_panel_and_back_restores_the_hub() {
    let plugin = spawn_settings(None).await;

    plugin
        .call("invoke", Some("settings"), Some(json!({})))
        .await
        .expect("invoke answered");

    let resp = plugin
        .call(
            "view.interact",
            Some("settings"),
            Some(json!({ "custom_id": "settings:about" })),
        )
        .await
        .expect("about click answered");

    assert_envelope(&resp, 1);
    let Msg::Resp {
        data: Some(data), ..
    } = &resp
    else {
        panic!("expected ok envelope, got {resp:?}");
    };
    let components = data["data"]["components"].as_array().expect("components");
    assert_eq!(components.len(), 2, "container plus the Back row");
    assert_eq!(
        components[1]["components"][0]["custom_id"],
        json!("settings:about:back")
    );
    let children = components[0]["components"].as_array().expect("children");
    assert_eq!(children.len(), 2, "section plus the license row");
    let section = &children[0];
    assert_eq!(section["type"], json!(9));
    let text = &section["components"][0];
    assert_eq!(text["type"], json!(10));
    assert!(
        text["content"]
            .as_str()
            .unwrap()
            .contains("Settings > About"),
        "the About copy names the page"
    );
    assert_eq!(section["accessory"]["type"], json!(2));
    assert_eq!(section["accessory"]["style"], json!(5), "link accessory");

    let back = plugin
        .call(
            "view.interact",
            Some("settings"),
            Some(json!({ "custom_id": "settings:about:back" })),
        )
        .await
        .expect("back click answered");
    let view = assert_envelope(&back, 2);
    assert_toggles(&view, false, false, false);

    let status = plugin.stop().await.expect("graceful stop");
    assert_eq!(status.code(), Some(0), "clean exit after bye: {status}");
}

/// An About click issues `host.stats` and renders the live values formatted
/// like `/about`'s Stats section — served by a mock source riding the real
/// wire, proving the host op reaches the plugin end to end.
#[tokio::test]
async fn about_click_renders_live_stats_from_the_host() {
    let mut source = MockStatsSource::new();
    source.expect_stats().times(1).returning(|| {
        Ok(HostStats {
            version: "9.9.9".into(),
            uptime_secs: 90_000,
            guild_count: 2,
            user_count: 1_500,
            latency_ms: 42,
            command_count: 12,
            memory_mb: 320.0,
        })
    });
    let services = stats_host_services(
        Arc::new(MockHostIo::new()),
        SharedKv::new(),
        InteractionEngine::new(),
        Arc::new(source),
    );
    let plugin = RunningPlugin::spawn_with(probe_binary("settings"), Some(services), None, None)
        .await
        .expect("spawn settings plugin");

    plugin
        .call("invoke", Some("settings"), Some(json!({})))
        .await
        .expect("invoke answered");

    let resp = plugin
        .call(
            "view.interact",
            Some("settings"),
            Some(json!({ "custom_id": "settings:about" })),
        )
        .await
        .expect("about click answered");
    let view = assert_envelope(&resp, 1);
    assert_page(&view, "about");

    let Msg::Resp {
        data: Some(data), ..
    } = &resp
    else {
        panic!("expected ok envelope, got {resp:?}");
    };
    let text = &data["data"]["components"][0]["components"][0]["components"][0];
    assert_eq!(text["type"], json!(10), "the copy is one text display");
    let content = text["content"].as_str().unwrap();
    for line in [
        "### Stats",
        "- **Uptime**: 1 days, 1 hours, 0 minutes",
        "- **Servers**: 2",
        "- **Users**: 1.5k",
        "- **Commands**: 12",
        "- **Latency**: 42ms",
        "- **Memory**: 320.0 MB",
        "Copyright © FAZuH — v9.9.9",
    ] {
        assert!(content.contains(line), "missing {line:?} in: {content}");
    }
    assert!(content.contains("### Info"), "Info section still renders");

    let status = plugin.stop().await.expect("graceful stop");
    assert_eq!(status.code(), Some(0), "clean exit after bye: {status}");
}

/// Asserts the envelope's `view` state names the given page.
fn assert_page(view: &Value, expected: &str) {
    assert_eq!(view["page"], json!(expected), "session page");
}

/// A config button click answers the hub unchanged:
/// routing to per-feature panels is future work, so the stub re-renders the
/// current page without touching the model.
#[tokio::test]
async fn a_config_button_answers_the_hub_without_changing_the_model() {
    let plugin = spawn_settings(None).await;

    plugin
        .call("invoke", Some("settings"), Some(json!({})))
        .await
        .expect("invoke answered");

    let resp = plugin
        .call(
            "view.interact",
            Some("settings"),
            Some(json!({ "custom_id": "settings:config:voice" })),
        )
        .await
        .expect("config click answered");

    let view = assert_envelope(&resp, 1);
    assert_toggles(&view, false, false, false);

    let status = plugin.stop().await.expect("graceful stop");
    assert_eq!(status.code(), Some(0), "clean exit after bye: {status}");
}

/// Two concurrently open hubs keep independent session state: an About click
/// on one flips only that session's page — the other still toggles on the
/// hub page, against its own model.
#[tokio::test]
async fn concurrent_hubs_keep_independent_pages() {
    let plugin = spawn_settings(None).await;

    let first_view = {
        let resp = plugin
            .call("invoke", Some("settings"), Some(json!({})))
            .await
            .expect("first invoke answered");
        assert_envelope(&resp, 0)
    };
    let second_view = {
        let resp = plugin
            .call("invoke", Some("settings"), Some(json!({})))
            .await
            .expect("second invoke answered");
        assert_envelope(&resp, 1)
    };

    // Session two goes to About; the host echoes its own view state back.
    let about_resp = plugin
        .call(
            "view.interact",
            Some("settings"),
            Some(json!({
                "custom_id": "settings:about",
                "view": second_view,
            })),
        )
        .await
        .expect("about click answered");
    let about_view = assert_envelope(&about_resp, 2);
    assert_page(&about_view, "about");

    // Session one never left the hub: its toggle answers the hub page with
    // the model of ITS session (voice on, feeds and welcome untouched).
    let toggle_resp = plugin
        .call(
            "view.interact",
            Some("settings"),
            Some(json!({
                "custom_id": "settings:toggle",
                "data": { "values": ["Voice"] },
                "view": first_view,
            })),
        )
        .await
        .expect("toggle answered");
    let toggled_view = assert_envelope(&toggle_resp, 3);
    assert_page(&toggled_view, "hub");
    assert_toggles(&toggled_view, false, true, false);

    let status = plugin.stop().await.expect("graceful stop");
    assert_eq!(status.code(), Some(0), "clean exit after bye: {status}");
}

/// A fresh `/settings` invoke always opens the hub, even right after an
/// About click left a session showing the About panel.
#[tokio::test]
async fn a_fresh_invoke_after_an_about_click_renders_the_hub() {
    let plugin = spawn_settings(None).await;

    let first_view = {
        let resp = plugin
            .call("invoke", Some("settings"), Some(json!({})))
            .await
            .expect("invoke answered");
        assert_envelope(&resp, 0)
    };
    let about_resp = plugin
        .call(
            "view.interact",
            Some("settings"),
            Some(json!({
                "custom_id": "settings:about",
                "view": first_view,
            })),
        )
        .await
        .expect("about click answered");
    let about_view = assert_envelope(&about_resp, 1);
    assert_page(&about_view, "about");

    let again_view = {
        let resp = plugin
            .call("invoke", Some("settings"), Some(json!({})))
            .await
            .expect("second invoke answered");
        assert_envelope(&resp, 2)
    };
    assert_page(&again_view, "hub");

    let status = plugin.stop().await.expect("graceful stop");
    assert_eq!(status.code(), Some(0), "clean exit after bye: {status}");
}
