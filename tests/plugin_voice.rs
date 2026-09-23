//! End-to-end tests for the voice settings panel plugin (#149, the
//! panel-migration's second panel): the panel plugin is spawned over the
//! real stdio wire — no database, no Discord — through the plugin manager,
//! like the core plugins the host spawns at startup. The voice settings seam
//! is served by a mockall mock of the host's [`VoiceSettingsSource`] and the
//! Discord I/O seam by a mock [`HostIo`].
//!
//! Assertions mirror the documented contract:
//! - the panel's invoke loads the guild's snapshot through
//!   `host.voice.get_settings` and renders the monolith `/vc settings`
//!   layout as Components V2;
//! - a plain edit (toggle) re-renders without a host call;
//! - Back persists the whole snapshot exactly once through
//!   `host.voice.update_settings`, then hands the message back to the host
//!   Settings GUI (the panel re-renders only when no live Settings session
//!   takes the message back);
//! - the engine's `view.timeout` event persists the last snapshot once,
//!   answering nothing;
//! - a failed settings load fails the open with the host's typed error
//!   forwarded;
//! - `bye` exits cleanly with status 0.

use std::path::PathBuf;
use std::sync::Arc;

use mockall::predicate::eq;
use pwr_bot::plugin::HostConfig;
use pwr_bot::plugin::HostServices;
use pwr_bot::plugin::PluginManager;
use pwr_bot::plugin::RespawnPolicy;
use pwr_bot::plugin::RunningPlugin;
use pwr_bot::plugin::StatsHandle;
use pwr_bot::plugin::VoiceSettingsError;
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

/// A settings snapshot with the voice field set, so wire round trips are
/// observable end to end.
fn sample_settings() -> ServerSettings {
    ServerSettings {
        voice: pwr_plugin_protocol::VoiceSettings {
            enabled: Some(true),
        },
        ..ServerSettings::default()
    }
}

/// The services the panel shares with its host calls, like the host's one
/// [`HostServices`] arc: the io seam posts placeholders and edits payloads,
/// and the voice seam serves the panel's settings RPCs.
fn services(
    io: Arc<MockHostIo>,
    voice: Arc<MockVoiceSettingsSource>,
    settings_returns: Option<Arc<pwr_bot::bot::translate::SettingsReturns>>,
) -> Arc<HostServices> {
    Arc::new(HostServices {
        io: Some(io),
        config: Some(HostConfig {
            db_url: "postgres://test".into(),
            data_path: PathBuf::from("/tmp/pwr-bot-test"),
            poll_interval: std::time::Duration::from_secs(30),
        }),
        kv: None,
        engine: None,
        stats: Arc::new(StatsHandle::default()),
        feeds: None,
        voice: Some(voice),
        welcome: None,
        previews: None,
        settings_returns,
    })
}

/// Spawns the panel plugin under a manager wired with the shared services,
/// like the host's startup loop spawns its core plugins.
async fn spawn_panel(services: Arc<HostServices>) -> Arc<RunningPlugin> {
    let manager =
        Arc::new(PluginManager::new(None, RespawnPolicy::default()).with_host_services(services));
    manager
        .spawn("voice", probe_binary("voice"), None, &[], &[])
        .await
        .expect("spawn voice panel")
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

/// The panel's status text display.
fn panel_status(resp: &Msg) -> &str {
    match resp {
        Msg::Resp {
            data: Some(data), ..
        } => data["data"]["components"][0]["components"][0]["content"]
            .as_str()
            .expect("status text display"),
        other => panic!("expected a resp, got {other:?}"),
    }
}

/// The full edit loop, then Back: the toggle re-renders without a host
/// call, Back persists the edited whole snapshot exactly once through
/// `host.voice.update_settings`, then hands the message back to the host
/// Settings GUI — the host-reserved `settings` open_view target completes
/// the waiter the parked session parked, and the panel answers its
/// interaction with the `ViewMoved` marker instead of a render.
#[tokio::test]
async fn open_edit_and_back_persist_the_snapshot_once_and_return() {
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

    let returns = Arc::new(pwr_bot::bot::translate::SettingsReturns::default());
    let panel = spawn_panel(services(
        Arc::new(MockHostIo::new()),
        Arc::new(voice),
        Some(returns.clone()),
    ))
    .await;

    // The invoke the Settings section handoff would issue — with the
    // forwarded guild id. The panel renders the active snapshot.
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
    assert!(panel_status(&resp).contains("**active**"));

    // A plain edit: the toggle flips the model with no host call — the
    // re-rendered panel shows the paused copy.
    let resp = panel
        .call(
            "view.interact",
            Some("voice-settings"),
            Some(json!({
                "custom_id": "voice:toggle",
                "view": view,
            })),
        )
        .await
        .expect("toggle answered");
    let view = assert_envelope(&resp, 1);
    assert!(panel_status(&resp).contains("**paused**"));

    // Back: persist once (the mock pins the toggled snapshot), then the
    // in-place open_view against the host-reserved `settings` target wakes
    // the parked session — the panel answers with the ViewMoved marker and
    // the host re-runs the Settings GUI on the message.
    let rx = returns.wait(poise::serenity_prelude::MessageId::new(777));
    let resp = panel
        .call(
            "view.interact",
            Some("voice-settings"),
            Some(json!({
                "custom_id": "voice:back",
                "view": view,
                "channel_id": 999,
                "message": { "id": "777" },
            })),
        )
        .await
        .expect("back answered");
    match &resp {
        Msg::Resp {
            ok: false,
            error: Some(err),
            ..
        } => assert_eq!(err.kind, "ViewMoved"),
        other => panic!("expected the ViewMoved marker, got {other:?}"),
    }
    rx.await.expect("the parked session was woken");
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

    let panel = spawn_panel(services(Arc::new(MockHostIo::new()), Arc::new(voice), None)).await;

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
/// forwarded: the Settings section handoff surfaces what the service said.
#[tokio::test]
async fn a_failed_load_fails_the_open_with_the_forwarded_error() {
    let mut voice = MockVoiceSettingsSource::new();
    voice
        .expect_get_settings()
        .with(eq(GUILD_ID))
        .times(1)
        .returning(|_| Err(VoiceSettingsError::Service(anyhow::anyhow!("guild gone"))));

    let panel = spawn_panel(services(Arc::new(MockHostIo::new()), Arc::new(voice), None)).await;

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
