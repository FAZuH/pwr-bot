//! End-to-end tests for the welcome settings panel plugin (#151, the
//! panel-migration's third panel): the hub and the panel plugin are spawned
//! over the real stdio wire — no database, no Discord — through one plugin
//! manager, like the two core plugins the host spawns at startup. The
//! welcome settings seam is served by a mockall mock of the host's
//! [`WelcomeSettingsSource`] and the Discord I/O seam by a mock [`HostIo`].
//!
//! Assertions mirror the documented contract:
//! - the hub's Welcome button (`settings:open:welcome-settings`) opens the
//!   panel plugin through `host.open_view`, forwarding the source `guild_id`;
//! - the panel's invoke loads the guild's snapshot through
//!   `host.welcome.get_settings` and renders the monolith `/welcome` panel as
//!   Components V2, declaring the preview attachment slot by filename
//!   (ADR-0012) while cards are enabled;
//! - every mutating click persists immediately through
//!   `host.welcome.update_settings` (the monolith's persist-on-every-change
//!   semantics) after re-reading the snapshot — the session echo carries only
//!   the guild id and the removal selection, never the settings;
//! - a modal trigger click opens its modal through `host.open_modal` and
//!   answers the click with the `ModalOpened` marker, so the host sends
//!   nothing (ADR-0011); the later `view.modal_submit` re-reads, persists the
//!   answer, and carries the stashed removal selection across the round trip;
//! - Back re-opens the hub and persists nothing;
//! - a failed load fails the open with the host's typed error forwarded;
//! - `bye` exits cleanly with status 0.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use async_trait::async_trait;
use mockall::predicate::eq;
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
use pwr_bot::plugin::WelcomeSettingsError;
use pwr_bot::plugin::WelcomeSettingsSource;
use pwr_bot::plugin::host::MockHostIo;
use pwr_bot::plugin::host::MockWelcomeSettingsSource;
use pwr_plugin_protocol::MODAL_OPENED_KIND;
use pwr_plugin_protocol::MODAL_SUBMIT_OP;
use pwr_plugin_protocol::Msg;
use pwr_plugin_protocol::ServerSettings;
use pwr_plugin_protocol::WelcomeSettings;
use pwr_poise_components::IS_COMPONENTS_V2;
use serde_json::Value;
use serde_json::json;

mod probe;
use probe::probe_binary;

/// The guild the panel keys its settings by.
const GUILD_ID: u64 = 42;

/// The preview filename the envelope declares, matching the host's
/// `WELCOME_FILE` (ADR-0012).
const WELCOME_FILE: &str = "welcome_preview.png";

/// A settings snapshot with the welcome section enabled and two messages, so
/// the toggle, the removal flow, and the attachment declaration are all
/// observable on the wire.
fn sample_settings() -> ServerSettings {
    ServerSettings {
        welcome: WelcomeSettings {
            enabled: Some(true),
            channel_id: Some("123456789".into()),
            primary_color: Some("#5865F2".into()),
            template_id: Some("1".into()),
            messages: Some(vec!["one".into(), "two".into()]),
        },
        ..ServerSettings::default()
    }
}

/// A stateful in-memory [`KvStore`], for the hub's load/persist chain.
#[derive(Default)]
struct SharedKv {
    inner: Mutex<HashMap<(String, String), String>>,
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

/// The services both core plugins share, like the host's one
/// [`HostServices`] arc: the io seam posts placeholders and edits payloads,
/// the engine tracks sessions, the kv store backs the hub, and the welcome
/// seam serves the panel's settings RPCs. No preview resolver: the
/// declaration passes through the transport untouched, which is what the
/// envelope assertions pin.
fn shared_services(
    io: Arc<dyn HostIo>,
    welcome: Arc<dyn WelcomeSettingsSource>,
    engine: InteractionEngine<RunningPlugin>,
) -> Arc<HostServices> {
    Arc::new(HostServices {
        io: Some(io),
        config: Some(HostConfig {
            db_url: "postgres://test".into(),
            data_path: PathBuf::from("/tmp/pwr-bot-test"),
            poll_interval: std::time::Duration::from_secs(30),
        }),
        kv: Some(Arc::new(SharedKv::default())),
        engine: Some(Arc::new(engine)),
        stats: Arc::new(StatsHandle::default()),
        feeds: None,
        voice: None,
        welcome: Some(welcome),
        previews: None,
    })
}

/// Spawns the hub and the panel plugin under one manager wired with the
/// shared services, like the host's startup loop spawns its core plugins.
async fn spawn_core_plugins(
    services: Arc<HostServices>,
) -> (Arc<PluginManager>, Arc<RunningPlugin>, Arc<RunningPlugin>) {
    let manager =
        Arc::new(PluginManager::new(None, RespawnPolicy::default()).with_host_services(services));
    let hub = manager
        .spawn("settings", probe_binary("settings"), None, &[], &[])
        .await
        .expect("spawn settings hub");
    let panel = manager
        .spawn(
            "welcome-settings",
            probe_binary("welcome-settings"),
            None,
            &[],
            &[],
        )
        .await
        .expect("spawn welcome-settings panel");
    (manager, hub, panel)
}

/// Asserts a resp is an ok view envelope answering its own invoke id, and
/// returns its `view` value.
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
            data["view"].clone()
        }
        _ => panic!("expected ok envelope, got {resp:?}"),
    }
}

/// The raw `data` of an ok resp, for content assertions.
fn resp_data(resp: &Msg) -> Value {
    match resp {
        Msg::Resp {
            data: Some(data), ..
        } => data.clone(),
        _ => panic!("expected ok envelope, got {resp:?}"),
    }
}

/// The panel's status text display.
fn panel_status(data: &Value) -> &str {
    data["data"]["components"][0]["components"][0]["content"]
        .as_str()
        .expect("status text display")
}

/// The panel's attachment declaration (ADR-0012), as the envelope carries it.
fn declared_attachments(data: &Value) -> &Value {
    &data["data"]["attachments"]
}

/// The hub's Welcome click, proven end to end: `host.open_view` resolves the
/// panel through the manager, forwards the source `guild_id`, and the panel's
/// invoke loads the guild's snapshot through the welcome seam before its
/// first render — the io mock pins the placeholder post and the final edit of
/// the panel onto the produced message, attachment declaration included
/// (previews unresolved here, so the slot rides the edit verbatim).
#[tokio::test]
async fn hub_welcome_click_opens_the_panel_with_the_guild_settings() {
    let channel_id = 555_000_111_u64;
    let produced = 777_000_222_u64;

    let mut welcome = MockWelcomeSettingsSource::new();
    welcome
        .expect_get_settings()
        .with(eq(GUILD_ID))
        .times(1)
        .returning(|_| Ok(sample_settings()));

    let mut io = MockHostIo::new();
    io.expect_send_message()
        .with(eq(channel_id), eq("Loading…"))
        .times(1)
        .returning(move |_, _| Ok(Some(json!({ "message_id": produced }))));
    io.expect_edit_message()
        .with(
            eq(channel_id),
            eq(produced),
            mockall::predicate::function(|data: &Value| {
                data["components"][0]["components"][0]["content"]
                    == json!("-# **Settings > Welcome**\n## Welcome Settings\n\n> 🛈  Welcome cards are **active**.")
                    && data["attachments"] == json!([{ "id": 0, "filename": WELCOME_FILE }])
            }),
            mockall::predicate::always(),
        )
        .times(1)
        .returning(|_, _, _, _| Ok(Some(json!({}))));

    let (_manager, hub, _panel) = spawn_core_plugins(shared_services(
        Arc::new(io),
        Arc::new(welcome),
        InteractionEngine::new(),
    ))
    .await;

    hub.call("invoke", Some("settings"), Some(json!({})))
        .await
        .expect("hub invoke answered");

    // The Welcome click: the rewired config button rides the nav id, and the
    // interaction carries the channel and guild like a real one does.
    let resp = hub
        .call(
            "view.interact",
            Some("settings"),
            Some(json!({
                "custom_id": "settings:open:welcome-settings",
                "channel_id": channel_id,
                "guild_id": GUILD_ID,
            })),
        )
        .await
        .expect("welcome click answered");

    // The hub answers the click with its own envelope (the hub stays the
    // hub); the panel rendered onto the produced message — the edit mock
    // above pins the payload, the get-settings mock pins the load.
    let view = assert_envelope(&resp, 1);
    assert_eq!(view["page"], json!("hub"));
}

/// The panel's first render, pinned on the wire: the invoke loads the
/// snapshot, the envelope carries the monolith's status copy, declares the
/// preview slot by filename while cards are enabled (ADR-0012), and echoes a
/// session state of guild id plus empty selection — never the settings.
#[tokio::test]
async fn invoke_renders_the_panel_and_declares_the_preview_slot() {
    let mut welcome = MockWelcomeSettingsSource::new();
    welcome
        .expect_get_settings()
        .with(eq(GUILD_ID))
        .times(1)
        .returning(|_| Ok(sample_settings()));

    let (_manager, _hub, panel) = spawn_core_plugins(shared_services(
        Arc::new(MockHostIo::new()),
        Arc::new(welcome),
        InteractionEngine::new(),
    ))
    .await;

    let resp = panel
        .call(
            "invoke",
            Some("welcome-settings"),
            Some(json!({ "guild_id": GUILD_ID })),
        )
        .await
        .expect("panel invoke answered");
    let view = assert_envelope(&resp, 0);
    assert_eq!(view, json!({ "guild_id": GUILD_ID, "marked_removal": [] }));
    let data = resp_data(&resp);
    assert!(panel_status(&data).contains("**active**"));
    assert_eq!(
        declared_attachments(&data),
        &json!([{ "id": 0, "filename": WELCOME_FILE }])
    );

    let status = panel.stop().await.expect("graceful stop");
    assert_eq!(status.code(), Some(0), "clean exit after bye: {status}");
}

/// A toggle click re-reads the snapshot, persists the flip immediately (the
/// monolith's persist-on-every-change semantics, unlike the voice panel's
/// persist-on-exit), and re-renders: the off copy shows and the attachment
/// declaration goes empty, which removes the preview on edit.
#[tokio::test]
async fn a_toggle_persists_immediately_and_renders_the_off_copy() {
    let mut off = sample_settings();
    off.welcome.enabled = Some(false);

    let mut welcome = MockWelcomeSettingsSource::new();
    welcome
        .expect_get_settings()
        .with(eq(GUILD_ID))
        .times(2)
        .returning(|_| Ok(sample_settings()));
    welcome
        .expect_update_settings()
        .with(eq(GUILD_ID), eq(off))
        .times(1)
        .returning(|_, _| Ok(()));

    let (_manager, _hub, panel) = spawn_core_plugins(shared_services(
        Arc::new(MockHostIo::new()),
        Arc::new(welcome),
        InteractionEngine::new(),
    ))
    .await;

    let resp = panel
        .call(
            "invoke",
            Some("welcome-settings"),
            Some(json!({ "guild_id": GUILD_ID })),
        )
        .await
        .expect("panel invoke answered");
    let view = assert_envelope(&resp, 0);

    let resp = panel
        .call(
            "view.interact",
            Some("welcome-settings"),
            Some(json!({
                "custom_id": "welcome:toggle",
                "view": view,
            })),
        )
        .await
        .expect("toggle answered");
    let view = assert_envelope(&resp, 1);
    assert_eq!(view, json!({ "guild_id": GUILD_ID, "marked_removal": [] }));
    let data = resp_data(&resp);
    assert!(panel_status(&data).contains("**disabled**"));
    assert_eq!(declared_attachments(&data), &json!([]));
}

/// A modal trigger click opens its modal through `host.open_modal` — the io
/// mock pins the nonce-suffixed custom id and the monolith's spec — and the
/// click is answered with the `ModalOpened` marker, so the host sends nothing
/// (ADR-0011). No settings load or persist rides a trigger click.
#[tokio::test]
async fn a_modal_trigger_opens_the_modal_and_answers_with_the_marker() {
    let opened = Arc::new(Mutex::new(None::<String>));
    let sink = opened.clone();

    let mut io = MockHostIo::new();
    io.expect_open_modal()
        .withf(move |interaction_id, token, modal| {
            *interaction_id == 9001 && token == "tok-1" && modal["title"] == "Add Welcome Message"
        })
        .times(1)
        .returning(move |_, _, modal| {
            *sink.lock().expect("open sink") = modal
                .get("custom_id")
                .and_then(Value::as_str)
                .map(str::to_string);
            Ok(())
        });

    let mut welcome = MockWelcomeSettingsSource::new();
    welcome
        .expect_get_settings()
        .with(eq(GUILD_ID))
        .times(1)
        .returning(|_| Ok(sample_settings()));

    let (_manager, _hub, panel) = spawn_core_plugins(shared_services(
        Arc::new(io),
        Arc::new(welcome),
        InteractionEngine::new(),
    ))
    .await;

    let resp = panel
        .call(
            "invoke",
            Some("welcome-settings"),
            Some(json!({ "guild_id": GUILD_ID })),
        )
        .await
        .expect("panel invoke answered");
    let view = assert_envelope(&resp, 0);

    let resp = panel
        .call(
            "view.interact",
            Some("welcome-settings"),
            Some(json!({
                "custom_id": "welcome:add",
                "id": 9001,
                "token": "tok-1",
                "user": { "id": 7 },
                "view": view,
            })),
        )
        .await
        .expect("trigger answered");
    match resp {
        Msg::Resp {
            id,
            ok: false,
            data: None,
            error: Some(err),
        } => {
            assert_eq!(id, 1);
            assert_eq!(err.kind, MODAL_OPENED_KIND);
        }
        other => panic!("expected the modal-opened marker, got {other:?}"),
    }

    let modal_id = opened
        .lock()
        .expect("open sink")
        .clone()
        .expect("the modal was opened");
    assert!(
        modal_id.starts_with("welcome:add:"),
        "the modal id is the trigger id, nonce-suffixed: {modal_id}"
    );
}

/// The full modal round trip: a marked removal survives the trigger (the
/// plugin stashes the selection under the modal id), the submission re-reads
/// the snapshot, persists the appended message through
/// `host.welcome.update_settings`, and answers with the re-rendered panel —
/// the selection still marked, the survivor list grown.
#[tokio::test]
async fn a_modal_submission_persists_the_answer_and_carries_the_stash() {
    let opened = Arc::new(Mutex::new(None::<String>));
    let sink = opened.clone();

    let mut io = MockHostIo::new();
    io.expect_open_modal()
        .times(1)
        .returning(move |_, _, modal| {
            *sink.lock().expect("open sink") = modal
                .get("custom_id")
                .and_then(Value::as_str)
                .map(str::to_string);
            Ok(())
        });

    let mut added = sample_settings();
    added
        .welcome
        .messages
        .as_mut()
        .expect("sample has messages")
        .push("three".into());

    let mut welcome = MockWelcomeSettingsSource::new();
    welcome
        .expect_get_settings()
        .with(eq(GUILD_ID))
        .times(3)
        .returning(|_| Ok(sample_settings()));
    welcome
        .expect_update_settings()
        .with(eq(GUILD_ID), eq(added))
        .times(1)
        .returning(|_, _| Ok(()));

    let (_manager, _hub, panel) = spawn_core_plugins(shared_services(
        Arc::new(io),
        Arc::new(welcome),
        InteractionEngine::new(),
    ))
    .await;

    let resp = panel
        .call(
            "invoke",
            Some("welcome-settings"),
            Some(json!({ "guild_id": GUILD_ID })),
        )
        .await
        .expect("panel invoke answered");
    let view = assert_envelope(&resp, 0);

    // Mark one message for removal: the selection lands in the session echo.
    let resp = panel
        .call(
            "view.interact",
            Some("welcome-settings"),
            Some(json!({
                "custom_id": "welcome:remove",
                "data": { "values": ["1"] },
                "view": view,
            })),
        )
        .await
        .expect("mark answered");
    let view = assert_envelope(&resp, 1);
    assert_eq!(view, json!({ "guild_id": GUILD_ID, "marked_removal": [1] }));

    // Open the add-message modal; the selection is stashed under its id.
    let resp = panel
        .call(
            "view.interact",
            Some("welcome-settings"),
            Some(json!({
                "custom_id": "welcome:add",
                "id": 9002,
                "token": "tok-2",
                "user": { "id": 7 },
                "view": view,
            })),
        )
        .await
        .expect("trigger answered");
    match resp {
        Msg::Resp { ok: false, .. } => {}
        other => panic!("expected the modal-opened marker, got {other:?}"),
    }
    let modal_id = opened
        .lock()
        .expect("open sink")
        .clone()
        .expect("the modal was opened");

    // The submission, in the shape the host delivers it: the raw interaction
    // with the bound custom id hoisted.
    let resp = panel
        .call(
            MODAL_SUBMIT_OP,
            None,
            Some(json!({
                "custom_id": modal_id,
                "guild_id": GUILD_ID,
                "data": {
                    "custom_id": modal_id,
                    "components": [{
                        "type": 18,
                        "component": { "custom_id": "message", "value": "three" }
                    }]
                }
            })),
        )
        .await
        .expect("submission answered");
    let view = assert_envelope(&resp, 3);
    assert_eq!(
        view,
        json!({ "guild_id": GUILD_ID, "marked_removal": [1] }),
        "the stashed selection carried across the modal round trip"
    );
    let data = resp_data(&resp);
    let options = data["data"]["components"][0]["components"][6]["components"][0]["options"]
        .as_array()
        .expect("removal select");
    assert_eq!(options.len(), 3, "the submitted message is listed");
    assert_eq!(options[1]["label"], json!("❌ two"), "still marked");
}

/// Marking persists nothing; saving drops the marked messages exactly once
/// and clears the marks from the session echo.
#[tokio::test]
async fn saving_removals_persists_the_survivors_once_and_clears_the_marks() {
    let mut survivors = sample_settings();
    survivors.welcome.messages = Some(vec!["two".into()]);

    let mut welcome = MockWelcomeSettingsSource::new();
    welcome
        .expect_get_settings()
        .with(eq(GUILD_ID))
        .times(3)
        .returning(|_| Ok(sample_settings()));
    welcome
        .expect_update_settings()
        .with(eq(GUILD_ID), eq(survivors))
        .times(1)
        .returning(|_, _| Ok(()));

    let (_manager, _hub, panel) = spawn_core_plugins(shared_services(
        Arc::new(MockHostIo::new()),
        Arc::new(welcome),
        InteractionEngine::new(),
    ))
    .await;

    let resp = panel
        .call(
            "invoke",
            Some("welcome-settings"),
            Some(json!({ "guild_id": GUILD_ID })),
        )
        .await
        .expect("panel invoke answered");
    let view = assert_envelope(&resp, 0);

    let resp = panel
        .call(
            "view.interact",
            Some("welcome-settings"),
            Some(json!({
                "custom_id": "welcome:remove",
                "data": { "values": ["0"] },
                "view": view,
            })),
        )
        .await
        .expect("mark answered");
    let view = assert_envelope(&resp, 1);
    assert_eq!(view, json!({ "guild_id": GUILD_ID, "marked_removal": [0] }));

    let resp = panel
        .call(
            "view.interact",
            Some("welcome-settings"),
            Some(json!({
                "custom_id": "welcome:save",
                "view": view,
            })),
        )
        .await
        .expect("save answered");
    let view = assert_envelope(&resp, 2);
    assert_eq!(view, json!({ "guild_id": GUILD_ID, "marked_removal": [] }));
}

/// Back re-opens the hub beside the panel and persists nothing: the terminal
/// messages hand off without a settings write (the persist-on-change already
/// happened on every earlier click).
#[tokio::test]
async fn back_reopens_the_hub_without_persisting() {
    let channel_id = 555_000_333_u64;
    let produced = 777_000_444_u64;

    let mut welcome = MockWelcomeSettingsSource::new();
    welcome
        .expect_get_settings()
        .with(eq(GUILD_ID))
        .times(2)
        .returning(|_| Ok(sample_settings()));
    welcome.expect_update_settings().never();

    let mut io = MockHostIo::new();
    io.expect_send_message()
        .with(eq(channel_id), eq("Loading…"))
        .times(1)
        .returning(move |_, _| Ok(Some(json!({ "message_id": produced }))));
    io.expect_edit_message()
        .times(1)
        .returning(|_, _, _, _| Ok(Some(json!({}))));

    let (_manager, _hub, panel) = spawn_core_plugins(shared_services(
        Arc::new(io),
        Arc::new(welcome),
        InteractionEngine::new(),
    ))
    .await;

    let resp = panel
        .call(
            "invoke",
            Some("welcome-settings"),
            Some(json!({ "guild_id": GUILD_ID })),
        )
        .await
        .expect("panel invoke answered");
    let view = assert_envelope(&resp, 0);

    let resp = panel
        .call(
            "view.interact",
            Some("welcome-settings"),
            Some(json!({
                "custom_id": "welcome:back",
                "channel_id": channel_id,
                "view": view,
            })),
        )
        .await
        .expect("back answered");
    let view = assert_envelope(&resp, 1);
    assert_eq!(view, json!({ "guild_id": GUILD_ID, "marked_removal": [] }));
}

/// A failed settings load fails the open with the host's typed error
/// forwarded: the hub's Welcome button shows what the service said.
#[tokio::test]
async fn a_failed_load_fails_the_open_with_the_forwarded_error() {
    let mut welcome = MockWelcomeSettingsSource::new();
    welcome
        .expect_get_settings()
        .with(eq(GUILD_ID))
        .times(1)
        .returning(|_| {
            Err(WelcomeSettingsError::Service(
                pwr_bot::service::error::ServiceError::UnexpectedResult {
                    message: "guild gone".into(),
                },
            ))
        });

    let (_manager, _hub, panel) = spawn_core_plugins(shared_services(
        Arc::new(MockHostIo::new()),
        Arc::new(welcome),
        InteractionEngine::new(),
    ))
    .await;

    let resp = panel
        .call(
            "invoke",
            Some("welcome-settings"),
            Some(json!({ "guild_id": GUILD_ID })),
        )
        .await
        .expect("invoke answered");
    match resp {
        Msg::Resp {
            ok: false,
            error: Some(err),
            ..
        } => {
            assert_eq!(err.kind, "WelcomeSettingsError");
            assert!(err.msg.contains("guild gone"), "msg: {}", err.msg);
        }
        _ => panic!("expected err resp, got {resp:?}"),
    }

    let status = panel.stop().await.expect("graceful stop");
    assert_eq!(status.code(), Some(0), "clean exit after bye: {status}");
}
