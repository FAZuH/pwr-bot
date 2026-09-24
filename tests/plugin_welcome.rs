//! End-to-end tests for the welcome settings panel plugin (#151, the
//! panel-migration's third panel): the panel plugin is spawned over the real
//! stdio wire — no database, no Discord — through the plugin manager, like
//! the core plugins the host spawns at startup. The welcome settings seam is
//! served by a mockall mock of the host's [`WelcomeSettingsSource`] and the
//! Discord I/O seam by a mock [`HostIo`].
//!
//! Assertions mirror the documented contract:
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
//! - a failed load fails the open with the host's typed error forwarded;
//! - `bye` exits cleanly with status 0.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use mockall::predicate::eq;
use pwr_bot::plugin::HostConfig;
use pwr_bot::plugin::HostIo;
use pwr_bot::plugin::HostServices;
use pwr_bot::plugin::InteractionEngine;
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

/// The services the panel shares with its host calls, like the host's one
/// [`HostServices`] arc: the io seam posts placeholders and edits payloads,
/// the engine backs the modal open, and the welcome seam serves the panel's
/// settings RPCs. No preview resolver: the
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
        kv: None,
        engine: Some(Arc::new(engine)),
        stats: Arc::new(StatsHandle::default()),
        voice: None,
        welcome: Some(welcome),
        previews: None,
        settings_returns: None,
    })
}

/// Spawns the panel plugin under a manager wired with the shared services,
/// like the host's startup loop spawns its core plugins.
async fn spawn_panel(services: Arc<HostServices>) -> Arc<RunningPlugin> {
    let manager =
        Arc::new(PluginManager::new(None, RespawnPolicy::default()).with_host_services(services));
    manager
        .spawn("welcome", probe_binary("welcome"), None, &[], &[])
        .await
        .expect("spawn welcome panel")
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

    let panel = spawn_panel(shared_services(
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

    let panel = spawn_panel(shared_services(
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

    let panel = spawn_panel(shared_services(
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

    let panel = spawn_panel(shared_services(
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

    let panel = spawn_panel(shared_services(
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

/// A failed settings load fails the open with the host's typed error
/// forwarded: the Settings section handoff surfaces what the service said.
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

    let panel = spawn_panel(shared_services(
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
