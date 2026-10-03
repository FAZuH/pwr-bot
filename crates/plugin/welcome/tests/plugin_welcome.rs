//! End-to-end tests for the welcome settings panel plugin (#151, the
//! panel-migration's third panel; #169, the plugin's own storage): the panel
//! is spawned over the real stdio and database seams — no Discord — through
//! the plugin manager, like the internal plugins the host spawns at startup. The
//! plugin reads and writes its own `welcome_settings` table and copies the
//! legacy `server_settings.settings.welcome` section into it once, on the
//! guild's first read.
//!
//! Assertions mirror the documented contract:
//! - the panel's invoke loads the guild's snapshot from plugin storage and
//!   renders the monolith `/welcome` panel as Components V2, shipping the
//!   preview image as a `files` entry while cards are enabled (ADR-0012) and
//!   never a `data.attachments` key;
//! - every mutating click persists immediately (the monolith's
//!   persist-on-every-change semantics), and the session echo carries only
//!   the guild id and the removal selection, never the settings;
//! - a modal trigger click opens its modal through `host.open_modal` and
//!   answers the click with the `ModalOpened` marker, so the host sends
//!   nothing (ADR-0011); the later `view.modal_submit` re-reads, persists the
//!   answer, and carries the stashed removal selection across the round trip;
//! - a failed load fails the open with `WelcomeSettingsError`;
//! - `bye` exits cleanly with status 0.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use pwr_bot::plugin::HostConfig;
use pwr_bot::plugin::HostIo;
use pwr_bot::plugin::HostServices;
use pwr_bot::plugin::InteractionEngine;
use pwr_bot::plugin::PluginManager;
use pwr_bot::plugin::RespawnPolicy;
use pwr_bot::plugin::RunningPlugin;
use pwr_bot::plugin::StatsHandle;
use pwr_bot::plugin::host::MockHostIo;
use pwr_bot::repo::PgRepos;
use pwr_plugin_protocol::MODAL_OPENED_KIND;
use pwr_plugin_protocol::MODAL_SUBMIT_OP;
use pwr_plugin_protocol::Msg;
use pwr_plugin_protocol::ServerSettings;
use pwr_plugin_protocol::WelcomeSettings;
use pwr_poise_components::IS_COMPONENTS_V2;
use pwr_test_support::db;
use serde_json::Value;
use serde_json::json;
mod probe;
use probe::probe_binary;

/// The preview filename the envelope ships (ADR-0012), matching the plugin's
/// `WELCOME_FILE`.
const WELCOME_FILE: &str = "welcome_preview.png";

/// The panel's command name, the `cmd` every invoke and click carries.
const COMMAND: &str = "welcome-settings";

/// A welcome snapshot with cards enabled and two messages, so the toggle,
/// the removal flow, and the preview file are all observable on the wire.
fn sample_welcome() -> WelcomeSettings {
    WelcomeSettings {
        enabled: Some(true),
        channel_id: Some("123456789".into()),
        primary_color: Some("#5865F2".into()),
        template_id: Some("1".into()),
        messages: Some(vec!["one".into(), "two".into()]),
    }
}

/// A per-process test database with the internal schema migrated and emptied.
/// The internal migration creates the `server_settings` table the panel copies
/// its legacy settings from on a guild's first read; the panel applies its own
/// migration at startup.
async fn database() -> String {
    let db_url = db::db_url().await;
    let core = PgRepos::new(&db_url).await.expect("connect core storage");
    core.run_migrations().await.expect("run core migrations");
    core.delete_all_tables().await.expect("clean core storage");
    db_url
}

/// A live test client, with its connection polled on the current runtime.
async fn connect(db_url: &str) -> tokio_postgres::Client {
    let (client, connection) = tokio_postgres::connect(db_url, tokio_postgres::NoTls)
        .await
        .expect("connect test client");
    tokio::spawn(async move {
        connection.await.expect("test client connection");
    });
    client
}

/// Seeds the legacy `server_settings` snapshot the plugin's first read
/// copies from.
async fn seed_legacy(client: &tokio_postgres::Client, guild_id: u64, welcome: WelcomeSettings) {
    let settings = ServerSettings {
        welcome,
        ..ServerSettings::default()
    };
    let payload = serde_json::to_string(&settings).expect("serialize legacy snapshot");
    client
        .execute(
            "INSERT INTO server_settings (guild_id, settings) VALUES ($1, $2::text::jsonb) \
             ON CONFLICT (guild_id) DO UPDATE SET settings = EXCLUDED.settings",
            &[&(guild_id as i64), &payload],
        )
        .await
        .expect("seed legacy welcome settings");
}

/// The services the panel shares with its host calls, like the host's one
/// [`HostServices`] arc: the io seam answers modal opens, the engine backs
/// `host.open_view`, and the config hands the plugin its database url.
fn services_with_io(
    io: Arc<dyn HostIo>,
    db_url: String,
    engine: InteractionEngine<RunningPlugin>,
) -> Arc<HostServices> {
    Arc::new(HostServices {
        io: Some(io),
        config: Some(HostConfig {
            db_url,
            data_path: PathBuf::from("/tmp/pwr-bot-welcome-test"),
            poll_interval: std::time::Duration::from_secs(30),
        }),
        kv: None,
        engine: Some(Arc::new(engine)),
        stats: Arc::new(StatsHandle::default()),
        users: Default::default(),
        settings_returns: None,
    })
}

/// Spawns the panel plugin under a manager wired with the shared services,
/// like the host's startup loop spawns its internal plugins.
async fn spawn_panel(io: Arc<dyn HostIo>, db_url: String) -> Arc<RunningPlugin> {
    let manager =
        Arc::new(
            PluginManager::new(None, RespawnPolicy::default())
                .with_host_services(services_with_io(io, db_url, InteractionEngine::new())),
        );
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
        _ => panic!("expected ok resp, got {resp:?}"),
    }
}

/// The panel's status text display.
fn panel_status(data: &Value) -> &str {
    data["data"]["components"][0]["components"][0]["content"]
        .as_str()
        .expect("status text display")
}

/// The preview files the envelope ships (ADR-0012).
fn preview_files(data: &Value) -> &Vec<Value> {
    data["files"].as_array().expect("envelope ships `files`")
}

/// The invoke loads the guild's snapshot — seeded from the legacy section on
/// this first read — and renders the monolith's panel: the status copy, the
/// session echo of guild id plus empty selection (never the settings), and
/// the preview image as one `files` entry while cards are enabled, with no
/// `data.attachments` declaration (the key itself rides empty, as serenity
/// serializes it).
#[tokio::test]
#[serial_test::serial]
async fn invoke_renders_the_panel_and_ships_the_preview_file() {
    let db_url = database().await;
    let client = connect(&db_url).await;
    seed_legacy(&client, 42, sample_welcome()).await;

    let panel = spawn_panel(Arc::new(MockHostIo::new()), db_url.clone()).await;
    let resp = panel
        .call("invoke", Some(COMMAND), Some(json!({ "guild_id": 42 })))
        .await
        .expect("panel invoke answered");
    let view = assert_envelope(&resp, 0);
    assert_eq!(view, json!({ "guild_id": 42, "marked_removal": [] }));
    let data = resp_data(&resp);
    assert!(panel_status(&data).contains("**active**"));
    let files = preview_files(&data);
    assert_eq!(files.len(), 1);
    assert_eq!(files[0]["filename"], json!(WELCOME_FILE));
    assert!(
        !files[0]["data_base64"]
            .as_str()
            .expect("preview bytes as base64")
            .is_empty(),
        "the shipped file carries renderable bytes"
    );
    let attachments = data["data"].get("attachments");
    assert!(
        attachments.is_none_or(|a| a.as_array().is_some_and(Vec::is_empty)),
        "no data.attachments declaration reaches Discord, got {attachments:?}"
    );

    // The first read copied the legacy section into the plugin's own table.
    let row = client
        .query_one(
            "SELECT enabled, messages::text FROM welcome_settings WHERE guild_id = $1",
            &[&42_i64],
        )
        .await
        .expect("read the copied snapshot");
    assert!(row.get::<_, bool>(0), "cards are enabled");
    let messages: Vec<String> = serde_json::from_str(row.get(1)).expect("message list");
    assert_eq!(messages, ["one", "two"]);

    let status = panel.stop().await.expect("graceful stop");
    assert_eq!(status.code(), Some(0), "clean exit after bye: {status}");
}

/// The copy is once-only: after the first read, editing the legacy section
/// must not change what the panel renders — later reads answer from
/// `welcome_settings` alone.
#[tokio::test]
#[serial_test::serial]
async fn the_legacy_section_is_copied_once_and_never_re_read() {
    let db_url = database().await;
    let client = connect(&db_url).await;
    seed_legacy(&client, 43, sample_welcome()).await;

    let panel = spawn_panel(Arc::new(MockHostIo::new()), db_url).await;
    let resp = panel
        .call("invoke", Some(COMMAND), Some(json!({ "guild_id": 43 })))
        .await
        .expect("first invoke answered");
    assert_envelope(&resp, 0);

    let edited = ServerSettings {
        welcome: WelcomeSettings {
            enabled: Some(false),
            channel_id: None,
            primary_color: None,
            template_id: None,
            messages: Some(vec!["stale".into()]),
        },
        ..ServerSettings::default()
    };
    let payload = serde_json::to_string(&edited).expect("serialize edited snapshot");
    client
        .execute(
            "UPDATE server_settings SET settings = $1::text::jsonb WHERE guild_id = $2",
            &[&payload, &43_i64],
        )
        .await
        .expect("edit the legacy section after the copy");

    let resp = panel
        .call("invoke", Some(COMMAND), Some(json!({ "guild_id": 43 })))
        .await
        .expect("second invoke answered");
    assert_envelope(&resp, 1);
    let data = resp_data(&resp);
    assert!(
        panel_status(&data).contains("**active**"),
        "the copied snapshot wins over the edited legacy section"
    );
    assert_eq!(
        preview_files(&data).len(),
        1,
        "cards still ship the preview"
    );

    panel.stop().await.expect("graceful stop");
}

/// A toggle click re-reads the snapshot, persists the flip immediately (the
/// monolith's persist-on-every-change semantics, unlike the voice panel's
/// persist-on-exit), and re-renders: the off copy shows and the envelope
/// ships no file, which removes the preview on edit.
#[tokio::test]
#[serial_test::serial]
async fn a_toggle_persists_immediately_and_renders_the_off_copy() {
    let db_url = database().await;
    let client = connect(&db_url).await;
    seed_legacy(&client, 44, sample_welcome()).await;

    let panel = spawn_panel(Arc::new(MockHostIo::new()), db_url).await;
    let resp = panel
        .call("invoke", Some(COMMAND), Some(json!({ "guild_id": 44 })))
        .await
        .expect("panel invoke answered");
    let view = assert_envelope(&resp, 0);
    assert_eq!(preview_files(&resp_data(&resp)).len(), 1);

    let resp = panel
        .call(
            "view.interact",
            Some(COMMAND),
            Some(json!({
                "custom_id": "welcome:toggle",
                "view": view,
            })),
        )
        .await
        .expect("toggle answered");
    let view = assert_envelope(&resp, 1);
    assert_eq!(view, json!({ "guild_id": 44, "marked_removal": [] }));
    let data = resp_data(&resp);
    assert!(panel_status(&data).contains("**disabled**"));
    assert!(
        preview_files(&data).is_empty(),
        "the preview is absent while disabled"
    );
    let attachments = data["data"].get("attachments");
    assert!(
        attachments.is_none_or(|a| a.as_array().is_some_and(Vec::is_empty)),
        "no data.attachments declaration reaches Discord, got {attachments:?}"
    );

    let row = client
        .query_one(
            "SELECT enabled FROM welcome_settings WHERE guild_id = $1",
            &[&44_i64],
        )
        .await
        .expect("read the persisted flip");
    assert!(!row.get::<_, bool>(0), "the flip persisted immediately");

    panel.stop().await.expect("graceful stop");
}

/// A modal trigger click opens its modal through `host.open_modal` — the io
/// mock pins the nonce-suffixed custom id and the monolith's spec — and the
/// click is answered with the `ModalOpened` marker, so the host sends nothing
/// (ADR-0011). No settings load or persist rides a trigger click.
#[tokio::test]
#[serial_test::serial]
async fn a_modal_trigger_opens_the_modal_and_answers_with_the_marker() {
    let db_url = database().await;
    let client = connect(&db_url).await;
    seed_legacy(&client, 45, sample_welcome()).await;

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

    let panel = spawn_panel(Arc::new(io), db_url).await;
    let resp = panel
        .call("invoke", Some(COMMAND), Some(json!({ "guild_id": 45 })))
        .await
        .expect("panel invoke answered");
    let view = assert_envelope(&resp, 0);

    let resp = panel
        .call(
            "view.interact",
            Some(COMMAND),
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

    panel.stop().await.expect("graceful stop");
}

/// The full modal round trip: a marked removal survives the trigger (the
/// plugin stashes the selection under the modal id), the submission re-reads
/// the snapshot, persists the appended message in the plugin's own storage,
/// and answers with the re-rendered panel — the selection still marked, the
/// survivor list grown.
#[tokio::test]
#[serial_test::serial]
async fn a_modal_submission_persists_the_answer_and_carries_the_stash() {
    let db_url = database().await;
    let client = connect(&db_url).await;
    seed_legacy(&client, 46, sample_welcome()).await;

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

    let panel = spawn_panel(Arc::new(io), db_url).await;
    let resp = panel
        .call("invoke", Some(COMMAND), Some(json!({ "guild_id": 46 })))
        .await
        .expect("panel invoke answered");
    let view = assert_envelope(&resp, 0);

    // Mark one message for removal: the selection lands in the session echo.
    let resp = panel
        .call(
            "view.interact",
            Some(COMMAND),
            Some(json!({
                "custom_id": "welcome:remove",
                "data": { "values": ["1"] },
                "view": view,
            })),
        )
        .await
        .expect("mark answered");
    let view = assert_envelope(&resp, 1);
    assert_eq!(view, json!({ "guild_id": 46, "marked_removal": [1] }));

    // Open the add-message modal; the selection is stashed under its id.
    let resp = panel
        .call(
            "view.interact",
            Some(COMMAND),
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
                "guild_id": 46,
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
        json!({ "guild_id": 46, "marked_removal": [1] }),
        "the stashed selection carried across the modal round trip"
    );
    let data = resp_data(&resp);
    let options = data["data"]["components"][0]["components"][6]["components"][0]["options"]
        .as_array()
        .expect("removal select");
    assert_eq!(options.len(), 3, "the submitted message is listed");
    assert_eq!(options[1]["label"], json!("❌ two"), "still marked");

    let row = client
        .query_one(
            "SELECT messages::text FROM welcome_settings WHERE guild_id = $1",
            &[&46_i64],
        )
        .await
        .expect("read the persisted message list");
    let messages: Vec<String> = serde_json::from_str(row.get(0)).expect("message list");
    assert_eq!(messages, ["one", "two", "three"]);

    panel.stop().await.expect("graceful stop");
}

/// Marking persists nothing; saving drops the marked messages exactly once
/// and clears the marks from the session echo.
#[tokio::test]
#[serial_test::serial]
async fn saving_removals_persists_the_survivors_and_clears_the_marks() {
    let db_url = database().await;
    let client = connect(&db_url).await;
    seed_legacy(&client, 47, sample_welcome()).await;

    let panel = spawn_panel(Arc::new(MockHostIo::new()), db_url).await;
    let resp = panel
        .call("invoke", Some(COMMAND), Some(json!({ "guild_id": 47 })))
        .await
        .expect("panel invoke answered");
    let view = assert_envelope(&resp, 0);

    let resp = panel
        .call(
            "view.interact",
            Some(COMMAND),
            Some(json!({
                "custom_id": "welcome:remove",
                "data": { "values": ["0"] },
                "view": view,
            })),
        )
        .await
        .expect("mark answered");
    let view = assert_envelope(&resp, 1);
    assert_eq!(view, json!({ "guild_id": 47, "marked_removal": [0] }));

    let resp = panel
        .call(
            "view.interact",
            Some(COMMAND),
            Some(json!({
                "custom_id": "welcome:save",
                "view": view,
            })),
        )
        .await
        .expect("save answered");
    let view = assert_envelope(&resp, 2);
    assert_eq!(view, json!({ "guild_id": 47, "marked_removal": [] }));

    let row = client
        .query_one(
            "SELECT messages::text FROM welcome_settings WHERE guild_id = $1",
            &[&47_i64],
        )
        .await
        .expect("read the persisted survivors");
    let messages: Vec<String> = serde_json::from_str(row.get(0)).expect("message list");
    assert_eq!(messages, ["two"], "the marked message was dropped");

    panel.stop().await.expect("graceful stop");
}

/// A failed settings load fails the open with the plugin's own typed error:
/// dropping `welcome_settings` out from under a running plugin surfaces
/// `WelcomeSettingsError` instead of a panel with no settings to edit.
#[tokio::test]
#[serial_test::serial]
async fn a_failed_load_fails_the_open_with_the_forwarded_error() {
    let db_url = database().await;
    let client = connect(&db_url).await;
    let panel = spawn_panel(Arc::new(MockHostIo::new()), db_url).await;

    // Wait for the plugin's own migration to land before breaking it.
    let mut ready = false;
    for _ in 0..100 {
        let exists: bool = client
            .query_one(
                "SELECT EXISTS (SELECT 1 FROM information_schema.tables \
                 WHERE table_schema = 'public' AND table_name = 'welcome_settings')",
                &[],
            )
            .await
            .expect("poll welcome migration")
            .get(0);
        if exists {
            ready = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(ready, "welcome plugin did not finish its migrations");

    let initial = panel
        .call("invoke", Some(COMMAND), Some(json!({ "guild_id": 48 })))
        .await
        .expect("invoke answered before the failure");
    assert_envelope(&initial, 0);

    client
        .execute("DROP TABLE welcome_settings", &[])
        .await
        .expect("break settings storage after plugin startup");

    let resp = panel
        .call("invoke", Some(COMMAND), Some(json!({ "guild_id": 48 })))
        .await;
    client
        .execute(
            "CREATE TABLE welcome_settings (guild_id BIGINT PRIMARY KEY, \
             enabled BOOLEAN NOT NULL DEFAULT FALSE, channel_id TEXT, \
             primary_color TEXT, template_id TEXT, messages JSONB)",
            &[],
        )
        .await
        .expect("restore settings storage");
    let resp = resp.expect("failed invoke answered");
    match resp {
        Msg::Resp {
            ok: false,
            error: Some(err),
            ..
        } => {
            assert_eq!(err.kind, "WelcomeSettingsError");
            assert!(err.msg.contains("welcome_settings"), "msg: {}", err.msg);
        }
        other => panic!("expected err resp, got {other:?}"),
    }

    let status = panel.stop().await.expect("graceful stop");
    assert_eq!(status.code(), Some(0), "clean exit after bye: {status}");
}
