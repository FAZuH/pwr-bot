use std::path::PathBuf;
use std::sync::Arc;

use pwr_bot::bot::translate::SettingsReturns;
use pwr_bot::plugin::HostConfig;
use pwr_bot::plugin::HostServices;
use pwr_bot::plugin::InteractionEngine;
use pwr_bot::plugin::PluginManager;
use pwr_bot::plugin::RespawnPolicy;
use pwr_bot::plugin::RunningPlugin;
use pwr_bot::plugin::StatsHandle;
use pwr_bot::plugin::host::MockHostIo;
use pwr_plugin_protocol::Msg;
use pwr_poise_components::IS_COMPONENTS_V2;
use serde_json::Value;
use serde_json::json;

mod common;
mod probe;
use probe::probe_binary;

async fn database() -> String {
    let db_url = common::db::db_url().await;
    let core = common::setup_db().await;
    common::teardown_db(&core).await;
    db_url
}

fn services(
    db_url: String,
    engine: InteractionEngine<RunningPlugin>,
    returns: Arc<SettingsReturns>,
) -> Arc<HostServices> {
    Arc::new(HostServices {
        io: Some(Arc::new(MockHostIo::new())),
        config: Some(HostConfig {
            db_url,
            data_path: PathBuf::from("/tmp/pwr-bot-voice-test"),
            poll_interval: std::time::Duration::from_secs(30),
        }),
        kv: None,
        engine: Some(Arc::new(engine)),
        stats: Arc::new(StatsHandle::default()),
        users: Default::default(),
        welcome: None,
        previews: None,
        settings_returns: Some(returns),
    })
}

async fn spawn_panel(db_url: String, returns: Arc<SettingsReturns>) -> Arc<RunningPlugin> {
    let engine = InteractionEngine::<RunningPlugin>::new();
    let host_services = services(db_url, engine, returns);
    let manager = Arc::new(
        PluginManager::new(None, RespawnPolicy::default()).with_host_services(host_services),
    );
    manager
        .spawn("voice", probe_binary("voice"), None, &[], &[])
        .await
        .expect("spawn voice plugin")
}

fn admin_args(guild_id: u64) -> Value {
    json!({
        "_context": {
            "user_id": 42,
            "guild_id": guild_id,
            "member_permissions": 8
        }
    })
}

fn assert_envelope(response: &Msg, expected_id: u64) -> Value {
    match response {
        Msg::Resp {
            id,
            ok: true,
            data: Some(data),
            error: None,
        } => {
            assert_eq!(*id, expected_id);
            assert_eq!(data["ephemeral"], json!(false));
            assert_eq!(data["data"]["flags"], json!(IS_COMPONENTS_V2));
            data["view"].clone()
        }
        other => panic!("expected an ok envelope, got {other:?}"),
    }
}

fn panel_status(response: &Msg) -> &str {
    match response {
        Msg::Resp {
            data: Some(data), ..
        } => data["data"]["components"][0]["components"][0]["content"]
            .as_str()
            .expect("status text"),
        other => panic!("expected a response, got {other:?}"),
    }
}

#[tokio::test]
#[serial_test::serial]
async fn open_edit_and_back_persist_the_snapshot_once_and_return() {
    let db_url = database().await;
    let returns = Arc::new(SettingsReturns::default());
    let panel = spawn_panel(db_url, returns.clone()).await;
    let guild_id = 42;

    let response = panel
        .call("invoke", Some("voice-settings"), Some(admin_args(guild_id)))
        .await
        .expect("invoke answered");
    let view = assert_envelope(&response, 0);
    assert!(panel_status(&response).contains("**active**"));

    let response = panel
        .call(
            "view.interact",
            Some("voice-settings"),
            Some(json!({
                "_context": admin_args(guild_id)["_context"],
                "custom_id": "voice:toggle",
                "view": view,
            })),
        )
        .await
        .expect("toggle answered");
    let view = assert_envelope(&response, 1);
    assert!(panel_status(&response).contains("**paused**"));

    let waiter = returns.wait(poise::serenity_prelude::MessageId::new(777));
    let response = panel
        .call(
            "view.interact",
            Some("voice-settings"),
            Some(json!({
                "_context": admin_args(guild_id)["_context"],
                "custom_id": "voice:back",
                "view": view,
                "channel_id": 999,
                "message": { "id": "777" },
            })),
        )
        .await
        .expect("back answered");
    assert!(matches!(
        response,
        Msg::Resp {
            ok: false,
            error: Some(error),
            ..
        } if error.kind == "ViewMoved"
    ));
    waiter.await.expect("settings waiter was woken");
    panel.stop().await.expect("stop voice plugin");
}

#[tokio::test]
#[serial_test::serial]
async fn about_persists_the_snapshot_once_and_opens_the_host_about_view() {
    let db_url = database().await;
    let returns = Arc::new(SettingsReturns::default());
    let panel = spawn_panel(db_url, returns.clone()).await;
    let guild_id = 43;

    let response = panel
        .call("invoke", Some("voice-settings"), Some(admin_args(guild_id)))
        .await
        .expect("invoke answered");
    let view = assert_envelope(&response, 0);
    let response = panel
        .call(
            "view.interact",
            Some("voice-settings"),
            Some(json!({
                "_context": admin_args(guild_id)["_context"],
                "custom_id": "voice:toggle",
                "view": view,
            })),
        )
        .await
        .expect("toggle answered");
    let view = assert_envelope(&response, 1);

    let waiter = returns.wait(poise::serenity_prelude::MessageId::new(778));
    let response = panel
        .call(
            "view.interact",
            Some("voice-settings"),
            Some(json!({
                "_context": admin_args(guild_id)["_context"],
                "custom_id": "voice:about",
                "view": view,
                "channel_id": 999,
                "message": { "id": "778" },
            })),
        )
        .await
        .expect("about answered");
    assert!(matches!(
        response,
        Msg::Resp {
            ok: false,
            error: Some(error),
            ..
        } if error.kind == "ViewMoved"
    ));
    assert_eq!(
        waiter.await.expect("settings waiter was woken"),
        pwr_bot::bot::translate::SettingsReturnPage::About
    );
    panel.stop().await.expect("stop voice plugin");
}

#[tokio::test]
#[serial_test::serial]
async fn expiry_persists_the_last_snapshot_once() {
    let db_url = database().await;
    let returns = Arc::new(SettingsReturns::default());
    let panel = spawn_panel(db_url.clone(), returns).await;
    panel
        .send_event(
            "view.timeout",
            Some(json!({
                "guild_id": 44,
                "settings": { "enabled": false }
            })),
        )
        .await
        .expect("timeout event delivered");
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let (client, connection) = tokio_postgres::connect(&db_url, tokio_postgres::NoTls)
        .await
        .expect("connect to verify settings");
    tokio::spawn(async move {
        connection.await.expect("verification connection");
    });
    let row = client
        .query_one(
            "SELECT enabled FROM voice_settings WHERE guild_id = $1",
            &[&44_i64],
        )
        .await
        .expect("read persisted settings");
    assert!(!row.get::<_, bool>(0));
    panel.stop().await.expect("stop voice plugin");
}

#[tokio::test]
#[serial_test::serial]
async fn a_failed_load_fails_the_open_with_the_forwarded_error() {
    let db_url = database().await;
    let returns = Arc::new(SettingsReturns::default());
    let panel = spawn_panel(db_url.clone(), returns).await;
    let (client, connection) = tokio_postgres::connect(&db_url, tokio_postgres::NoTls)
        .await
        .expect("connect to break settings storage");
    tokio::spawn(async move {
        connection.await.expect("settings failure connection");
    });
    let mut ready = false;
    for _ in 0..100 {
        let exists: bool = client
            .query_one(
                "SELECT EXISTS (SELECT 1 FROM information_schema.tables \
                 WHERE table_schema = 'public' AND table_name = 'voice_settings')",
                &[],
            )
            .await
            .expect("poll voice migration")
            .get(0);
        if exists {
            ready = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(ready, "voice plugin did not finish its migrations");
    let initial = panel
        .call("invoke", Some("voice-settings"), Some(admin_args(46)))
        .await
        .expect("settings invocation before failure");
    assert_envelope(&initial, 0);
    client
        .execute("DROP TABLE voice_settings", &[])
        .await
        .expect("break settings storage after plugin startup");

    let response = panel
        .call("invoke", Some("voice-settings"), Some(admin_args(46)))
        .await;
    client
        .execute(
            concat!(
                "CREATE TABLE voice_settings (guild_id BIGINT PRIMARY KEY, ",
                "enabled BOOLEAN NOT NULL DEFAULT TRUE)"
            ),
            &[],
        )
        .await
        .expect("restore settings storage");
    let response = response.expect("failed settings invocation answered");
    assert!(
        matches!(
            &response,
            Msg::Resp {
                ok: false,
                error: Some(error),
                ..
            } if error.kind == "CommandError"
                && error.msg.contains("voice service failed: database error")
                && error.msg.contains("relation \"voice_settings\" does not exist")
        ),
        "unexpected response: {response:?}"
    );
    panel.stop().await.expect("stop voice plugin");
}

#[tokio::test]
#[serial_test::serial]
async fn invalid_settings_actor_is_rejected() {
    let db_url = database().await;
    let returns = Arc::new(SettingsReturns::default());
    let panel = spawn_panel(db_url, returns).await;
    let response = panel
        .call(
            "invoke",
            Some("voice-settings"),
            Some(json!({
                "_context": {
                    "user_id": 42,
                    "guild_id": 45,
                    "member_permissions": 0
                }
            })),
        )
        .await
        .expect("invalid invocation answered");
    assert!(matches!(
        response,
        Msg::Resp {
            ok: false,
            error: Some(error),
            ..
        } if error.kind == "CommandError" && error.msg.contains("Manage Server")
    ));
    panel.stop().await.expect("stop voice plugin");
}
