//! End-to-end tests for the voice settings panel plugin (#149, the
//! panel-migration's second panel): the hub and the panel plugin are spawned
//! over the real stdio wire — no database, no Discord — through one plugin
//! manager, like the two core plugins the host spawns at startup. The voice
//! settings seam is served by a mockall mock of the host's
//! [`VoiceSettingsSource`] and the Discord I/O seam by a mock [`HostIo`].
//!
//! Assertions mirror the documented contract:
//! - the hub's Voice button (`settings:open:voice-settings`) opens the panel
//!   plugin through `host.open_view`, forwarding the source `guild_id`;
//! - the panel's invoke loads the guild's snapshot through
//!   `host.voice.get_settings` and renders the monolith `/vc settings`
//!   layout as Components V2;
//! - a plain edit (toggle) re-renders without a host call;
//! - Back and About persist the whole snapshot exactly once through
//!   `host.voice.update_settings`, then re-open the hub (About asks for the
//!   hub's About page by name through the invoke args);
//! - the engine's `view.timeout` event persists the last snapshot once,
//!   answering nothing;
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
use pwr_bot::plugin::VoiceSettingsError;
use pwr_bot::plugin::VoiceSettingsSource;
use pwr_bot::plugin::host::MockHostIo;
use pwr_bot::plugin::host::MockVoiceSettingsSource;
use pwr_plugin_protocol::Msg;
use pwr_plugin_protocol::ServerSettings;
use pwr_poise_components::IS_COMPONENTS_V2;
use serde_json::Value;
use serde_json::json;

mod probe;
use probe::probe_binary;

/// The guild the panels key their settings by.
const GUILD_ID: u64 = 42;

/// A settings snapshot with the voice section set, so wire round trips are
/// observable end to end.
fn sample_settings() -> ServerSettings {
    ServerSettings {
        voice: pwr_plugin_protocol::VoiceSettings {
            enabled: Some(true),
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
/// the engine tracks sessions, the kv store backs the hub, and the voice
/// seam serves the panel's settings RPCs.
fn shared_services(
    io: Arc<dyn HostIo>,
    voice: Arc<dyn VoiceSettingsSource>,
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
        voice: Some(voice),
        welcome: None,
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
            "voice-settings",
            probe_binary("voice-settings"),
            None,
            &[],
            &[],
        )
        .await
        .expect("spawn voice-settings panel");
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

/// The hub's Voice click, proven end to end: `host.open_view` resolves the
/// panel through the manager, forwards the source `guild_id`, and the
/// panel's invoke loads the guild's snapshot through the voice seam before
/// its first render — the io mock pins the placeholder post and the final
/// edit of the panel onto the produced message.
#[tokio::test]
async fn hub_voice_click_opens_the_panel_with_the_guild_settings() {
    let channel_id = 555_000_111_u64;
    let produced = 777_000_222_u64;

    let mut voice = MockVoiceSettingsSource::new();
    voice
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
                    == json!("-# **Settings > Voice**\n## Voice Tracking Settings\n\n> 🛈  Voice tracking is **active**.")
            }),
            mockall::predicate::always(),
        )
        .times(1)
        .returning(|_, _, _, _| Ok(Some(json!({}))));

    let (_manager, hub, _panel) = spawn_core_plugins(shared_services(
        Arc::new(io),
        Arc::new(voice),
        InteractionEngine::new(),
    ))
    .await;

    hub.call("invoke", Some("settings"), Some(json!({})))
        .await
        .expect("hub invoke answered");

    // The Voice click: the rewired config button rides the nav id, and the
    // interaction carries the channel and guild like a real one does.
    let resp = hub
        .call(
            "view.interact",
            Some("settings"),
            Some(json!({
                "custom_id": "settings:open:voice-settings",
                "channel_id": channel_id,
                "guild_id": GUILD_ID,
            })),
        )
        .await
        .expect("voice click answered");

    // The hub answers the click with its own envelope (the hub stays the
    // hub); the panel rendered onto the produced message — the edit mock
    // above pins the payload, the get-settings mock pins the load.
    let view = assert_envelope(&resp, 1);
    assert_eq!(view["page"], json!("hub"));
}

/// The production interaction shape, pinned: the host merges a real
/// interaction into the `view.interact` args, and serenity's ids ride that
/// payload as strings. The same Voice click with string-form `channel_id`
/// and `guild_id` opens the panel for the same guild — the hub's id parsing
/// accepts numeric-or-string, so the forwarded invoke args and the
/// placeholder post carry the numeric ids unchanged.
#[tokio::test]
async fn hub_voice_click_with_string_ids_opens_the_panel_with_the_guild_settings() {
    let channel_id = 555_000_777_u64;
    let produced = 777_000_888_u64;

    let mut voice = MockVoiceSettingsSource::new();
    voice
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
        .times(1)
        .returning(|_, _, _, _| Ok(Some(json!({}))));

    let (_manager, hub, _panel) = spawn_core_plugins(shared_services(
        Arc::new(io),
        Arc::new(voice),
        InteractionEngine::new(),
    ))
    .await;

    hub.call("invoke", Some("settings"), Some(json!({})))
        .await
        .expect("hub invoke answered");

    let resp = hub
        .call(
            "view.interact",
            Some("settings"),
            Some(json!({
                "custom_id": "settings:open:voice-settings",
                "channel_id": channel_id.to_string(),
                "guild_id": GUILD_ID.to_string(),
            })),
        )
        .await
        .expect("voice click answered");

    let view = assert_envelope(&resp, 1);
    assert_eq!(view["page"], json!("hub"));
}

/// The full edit loop, then Back: the toggle re-renders without a host call,
/// Back persists the edited whole snapshot exactly once through
/// `host.voice.update_settings`, then re-opens the hub beside the panel —
/// the panel answers its own interaction with its own envelope, as every
/// plugin→plugin navigation does.
#[tokio::test]
async fn open_edit_and_back_persist_the_snapshot_once_and_reopen_the_hub() {
    let channel_id = 555_000_333_u64;
    let produced = 777_000_444_u64;

    let mut voice = MockVoiceSettingsSource::new();
    voice
        .expect_get_settings()
        .with(eq(GUILD_ID))
        .times(1)
        .returning(|_| Ok(sample_settings()));
    let mut toggled = sample_settings();
    toggled.voice.enabled = Some(false);
    voice
        .expect_update_settings()
        .with(eq(GUILD_ID), eq(toggled))
        .times(1)
        .returning(|_, _| Ok(()));

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
        Arc::new(voice),
        InteractionEngine::new(),
    ))
    .await;

    // The invoke the hub's open_view would issue — with the forwarded
    // guild id. The panel renders the active snapshot.
    let resp = panel
        .call(
            "invoke",
            Some("voice-settings"),
            Some(json!({ "guild_id": GUILD_ID })),
        )
        .await
        .expect("panel invoke answered");
    let view = assert_envelope(&resp, 0);
    assert_eq!(view["guild_id"], json!(GUILD_ID));
    assert!(panel_status(&resp_data(&resp)).contains("**active**"));

    // A plain edit: the toggle flips the model with no host call — the
    // re-rendered panel shows the paused copy.
    let resp = panel
        .call(
            "view.interact",
            Some("voice-settings"),
            Some(json!({
                "custom_id": "voice:toggle",
                "channel_id": channel_id,
                "view": view,
            })),
        )
        .await
        .expect("toggle answered");
    let view = assert_envelope(&resp, 1);
    assert!(panel_status(&resp_data(&resp)).contains("**paused**"));

    // Back: persist once (the mock pins the toggled snapshot), then re-open
    // the hub through the manager — the io mock pins the hub's placeholder
    // post on the source channel.
    let resp = panel
        .call(
            "view.interact",
            Some("voice-settings"),
            Some(json!({
                "custom_id": "voice:back",
                "channel_id": channel_id,
                "view": view,
            })),
        )
        .await
        .expect("back answered");
    let view = assert_envelope(&resp, 2);
    assert_eq!(view["guild_id"], json!(GUILD_ID));
}

/// About persists once, then opens the hub on its About page: the page name
/// rides the open_view invoke args the hub seeds its session from, exactly
/// like the monolith's `Navigation::SettingsAbout` handoff.
#[tokio::test]
async fn about_persists_and_opens_the_hub_on_its_about_page() {
    let channel_id = 555_000_555_u64;
    let produced = 777_000_666_u64;

    let mut voice = MockVoiceSettingsSource::new();
    voice
        .expect_get_settings()
        .with(eq(GUILD_ID))
        .times(1)
        .returning(|_| Ok(sample_settings()));
    voice
        .expect_update_settings()
        .with(eq(GUILD_ID), eq(sample_settings()))
        .times(1)
        .returning(|_, _| Ok(()));

    // The io seam pins the hub's About-page edit: the settings plugin is a
    // core plugin under the same manager, so the panel's open_view resolves
    // it and edits the hub's payload onto the produced message — a v2
    // payload whose copy names the About page.
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
                let text = &data["components"][0]["components"][0]["components"][0]["content"];
                text.as_str()
                    .is_some_and(|text| text.contains("Settings > About"))
            }),
            mockall::predicate::always(),
        )
        .times(1)
        .returning(|_, _, _, _| Ok(Some(json!({}))));

    let (_manager, _hub, panel) = spawn_core_plugins(shared_services(
        Arc::new(io),
        Arc::new(voice),
        InteractionEngine::new(),
    ))
    .await;

    let resp = panel
        .call(
            "invoke",
            Some("voice-settings"),
            Some(json!({ "guild_id": GUILD_ID })),
        )
        .await
        .expect("panel invoke answered");
    let view = assert_envelope(&resp, 0);

    let resp = panel
        .call(
            "view.interact",
            Some("voice-settings"),
            Some(json!({
                "custom_id": "voice:about",
                "channel_id": channel_id,
                "view": view,
            })),
        )
        .await
        .expect("about answered");
    assert_envelope(&resp, 1);
}

/// The expiry event persists the last snapshot exactly once, answering
/// nothing: the engine pushes `view.timeout` with the session's state.
#[tokio::test]
async fn expiry_persists_the_last_snapshot_once() {
    let mut voice = MockVoiceSettingsSource::new();
    voice
        .expect_update_settings()
        .with(eq(GUILD_ID), eq(sample_settings()))
        .times(1)
        .returning(|_, _| Ok(()));

    let (_manager, _hub, panel) = spawn_core_plugins(shared_services(
        Arc::new(MockHostIo::new()),
        Arc::new(voice),
        InteractionEngine::new(),
    ))
    .await;

    panel
        .send_event(
            "view.timeout",
            Some(json!({
                "guild_id": GUILD_ID,
                "settings": sample_settings(),
            })),
        )
        .await
        .expect("event delivered");

    // The persist is fire-and-forget: give the plugin's loop a beat to
    // resolve it before the bye, so the mock's times(1) expectation is
    // observed at drop.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let status = panel.stop().await.expect("graceful stop");
    assert_eq!(status.code(), Some(0), "clean exit after bye: {status}");
}

/// A failed settings load fails the open with the host's typed error
/// forwarded: the hub's Voice button shows what the service said.
#[tokio::test]
async fn a_failed_load_fails_the_open_with_the_forwarded_error() {
    let mut voice = MockVoiceSettingsSource::new();
    voice
        .expect_get_settings()
        .with(eq(GUILD_ID))
        .times(1)
        .returning(|_| Err(VoiceSettingsError::Service(anyhow::anyhow!("guild gone"))));

    let (_manager, _hub, panel) = spawn_core_plugins(shared_services(
        Arc::new(MockHostIo::new()),
        Arc::new(voice),
        InteractionEngine::new(),
    ))
    .await;

    let resp = panel
        .call(
            "invoke",
            Some("voice-settings"),
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
            assert_eq!(err.kind, "VoiceSettingsError");
            assert!(err.msg.contains("guild gone"), "msg: {}", err.msg);
        }
        _ => panic!("expected err resp, got {resp:?}"),
    }

    let status = panel.stop().await.expect("graceful stop");
    assert_eq!(status.code(), Some(0), "clean exit after bye: {status}");
}
