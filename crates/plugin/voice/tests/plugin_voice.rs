use std::path::PathBuf;
use std::sync::Arc;

use poise::serenity_prelude as serenity;
use pwr_bot::bot::command::session_exit::adopt_message_into_section;
use pwr_bot::bot::gui::GuiFeature;
use pwr_bot::bot::gui::settings::SettingsConfig;
use pwr_bot::bot::gui::settings::SettingsFeature;
use pwr_bot::bot::navigation::Navigation;
use pwr_bot::bot::translate::SettingsReturnPage;
use pwr_bot::bot::translate::SettingsReturns;
use pwr_bot::bot::view::ActionRegistry;
use pwr_bot::bot::view::SelectValues;
use pwr_bot::plugin::HostConfig;
use pwr_bot::plugin::HostServices;
use pwr_bot::plugin::InteractionEngine;
use pwr_bot::plugin::InteractionError;
use pwr_bot::plugin::PluginManager;
use pwr_bot::plugin::RespawnPolicy;
use pwr_bot::plugin::RunningPlugin;
use pwr_bot::plugin::StatsHandle;
use pwr_bot::plugin::host::MockHostIo;
use pwr_bot::plugin::validate_view_spec;
use pwr_bot::repo::PgRepos;
use pwr_bot::update::settings::SettingsSection;
use pwr_plugin_protocol::Msg;
use pwr_poise_components::IS_COMPONENTS_V2;
use pwr_test_support::db;
use serde_json::Value;
use serde_json::json;
mod probe;
use probe::probe_binary;

/// A per-process database with the core schema migrated and emptied. The core
/// migration creates the `server_settings` table the panel imports its legacy
/// settings from; the panel applies its own migration at startup.
async fn database() -> String {
    let db_url = db::db_url().await;
    let core = PgRepos::new(&db_url).await.expect("connect core storage");
    core.run_migrations().await.expect("run core migrations");
    core.delete_all_tables().await.expect("clean core storage");
    db_url
}

fn services(
    db_url: String,
    engine: InteractionEngine<RunningPlugin>,
    returns: Arc<SettingsReturns>,
    io: Arc<MockHostIo>,
) -> Arc<HostServices> {
    Arc::new(HostServices {
        io: Some(io),
        config: Some(HostConfig {
            db_url,
            data_path: PathBuf::from("/tmp/pwr-bot-voice-test"),
            poll_interval: std::time::Duration::from_secs(30),
        }),
        kv: None,
        engine: Some(Arc::new(engine)),
        stats: Arc::new(StatsHandle::default()),
        users: Default::default(),
        settings_returns: Some(returns),
    })
}

async fn spawn_panel(db_url: String, returns: Arc<SettingsReturns>) -> Arc<RunningPlugin> {
    spawn_panel_with_io(db_url, returns, Arc::new(MockHostIo::new()))
        .await
        .0
}

/// The panel plus the engine the host services share, so a test can drive
/// the same [`InteractionEngine`] the Discord event handler routes clicks
/// through rather than calling the plugin directly.
async fn spawn_panel_with_io(
    db_url: String,
    returns: Arc<SettingsReturns>,
    io: Arc<MockHostIo>,
) -> (Arc<RunningPlugin>, Arc<InteractionEngine<RunningPlugin>>) {
    let engine = Arc::new(InteractionEngine::<RunningPlugin>::new());
    let host_services = services(db_url, (*engine).clone(), returns, io);
    let manager = Arc::new(
        PluginManager::new(None, RespawnPolicy::default()).with_host_services(host_services),
    );
    let panel = manager
        .spawn("voice", probe_binary("voice"), None, &[], &[])
        .await
        .expect("spawn voice plugin");
    (panel, engine)
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

/// A panel reached through `/settings voice` is handed a message the host
/// parked a Settings session on, so Back has somewhere to return to. This
/// drives that whole path the way the bot does: park the waiter, adopt the
/// message into the panel, then route the Back press through the
/// `InteractionEngine` the global event handler uses.
///
/// The reported bug was a Back press that did nothing and logged
/// `NoSettingsSession: no live host Settings session is waiting on this
/// message` — the panel's `host.open_view` against the host-reserved
/// `settings` target found no waiter. So this asserts the press comes back
/// as `ViewMoved`, the parked waiter is woken with the Settings page, and
/// the engine session is released: the message is the host's again, free for
/// its settings re-render. A panel that answered with a wire error instead
/// leaves all three false.
#[tokio::test]
#[serial_test::serial]
async fn back_on_a_panel_handed_off_by_settings_returns_the_message_to_the_host() {
    let db_url = database().await;
    let returns = Arc::new(SettingsReturns::default());
    let guild_id = 47;
    let channel_id = 999;
    let message_id = 779;

    let mut io = MockHostIo::new();
    io.expect_edit_message()
        .times(1)
        .returning(move |_, _, _, _| Ok(Some(json!({ "message_id": message_id }))));
    let io = Arc::new(io);
    let (panel, engine) = spawn_panel_with_io(db_url, returns.clone(), io.clone()).await;

    // The handoff, in the order `Router::handoff_and_wait` does it: park the
    // waiter on the live message, then adopt that message into the panel.
    let waiter = returns.wait(serenity::MessageId::new(message_id));
    let mut message = serenity::Message::default();
    message.channel_id = serenity::ChannelId::new(channel_id).into();
    message.id = serenity::MessageId::new(message_id);
    adopt_message_into_section(
        &engine,
        panel.clone(),
        "voice-settings",
        json!({
            "guild_id": guild_id,
            "_context": admin_args(guild_id)["_context"],
        }),
        io.as_ref(),
        &message,
    )
    .await
    .expect("the settings handoff adopts the message into the panel");
    assert!(
        engine.has_session(message.id).await,
        "the adopted message carries a panel session for the click to reach"
    );

    let back = engine
        .interact_validated(
            message.id,
            "voice:back",
            json!({
                "_context": admin_args(guild_id)["_context"],
                "channel_id": channel_id,
                "message": { "id": message_id.to_string() },
            }),
            validate_view_spec,
        )
        .await;

    assert!(
        matches!(
            &back,
            Err(InteractionError::PluginRejected { kind, .. }) if kind == "ViewMoved"
        ),
        "Back hands the message to the host page instead of failing the \
         `host.open_view` call: {back:?}"
    );
    assert_eq!(
        waiter.await.expect("the parked settings waiter was woken"),
        SettingsReturnPage::Settings,
        "the wake-up names the host page to re-run on this message"
    );
    assert!(
        !engine.has_session(message.id).await,
        "the panel session is released, so the host's settings re-render \
         does not race a live plugin session"
    );
    panel.stop().await.expect("stop voice plugin");
}

/// A panel is reachable only through `/settings <plugin>`: the manifest's
/// settings section names the plugin command that opens it, and that command
/// is deliberately not a slash command, so nothing syncs it to Discord and
/// nothing but this path can reach it. A manifest validates fine and still
/// dispatches to nothing, so the manifest is not evidence.
///
/// This drives the whole way a user does: render the `/settings` page from the
/// manifest's declared section, press the tile the page actually renders, take
/// the navigation that press exits with, and hand the message to that
/// navigation's plugin and command through the same adoption seam the Router
/// uses. The command is read from the manifest and never written here, so a
/// tile that dispatches the wrong command fails.
///
/// What proves the plugin received the invoke is the panel the plugin rendered
/// arriving at Discord: the `MockHostIo` edit body is the plugin's own
/// `ViewSpec` payload, carrying plugin-authored text the host has no way to
/// synthesize. The closing call is the falsifier — the same plugin, through the
/// same engine, handed a command the manifest does not declare, refuses it and
/// quotes back the command it was given. So the plugin demonstrably reads the
/// `cmd` off the invoke, and the panel above can only have come from the
/// declared command reaching the panel handler.
#[tokio::test]
#[serial_test::serial]
async fn a_settings_tile_dispatches_the_manifests_panel_command_to_the_plugin() {
    let db_url = database().await;
    let returns = Arc::new(SettingsReturns::default());
    let guild_id = 51;
    let channel_id = 995;
    let message_id = 782;

    // The body the plugin's panel is morphed into, so the assertion can read
    // the plugin's own render rather than a host-side success value.
    let edited: Arc<std::sync::Mutex<Option<Value>>> = Arc::new(std::sync::Mutex::new(None));
    let captured = edited.clone();
    let mut io = MockHostIo::new();
    io.expect_edit_message()
        .times(1)
        .returning(move |_, _, data, _| {
            *captured.lock().expect("edit body lock") = Some(data);
            Ok(Some(json!({ "message_id": message_id })))
        });
    let io = Arc::new(io);
    let (panel, engine) = spawn_panel_with_io(db_url, returns.clone(), io.clone()).await;

    // The `/settings` page, assembled from the running plugin's manifest
    // exactly as `settings::gather_sections` assembles it.
    let manifest = panel.manifest().expect("voice hello carried its manifest");
    let declared = manifest
        .settings
        .first()
        .expect("voice declares a settings section")
        .clone();
    let plugin_name = manifest.name.clone();
    let sections = manifest
        .settings
        .iter()
        .map(|section| SettingsSection::from((plugin_name.clone(), section.clone())))
        .collect();
    let model = SettingsFeature::initial(SettingsConfig { sections });

    // The tile as the page renders it: the button labelled with the section's
    // display name, carrying the custom id Discord would hand back.
    let mut registry = ActionRegistry::new();
    let page = serde_json::to_value(SettingsFeature::view(&model, &mut registry))
        .expect("settings page serializes");
    let tile = find_button_id(&page, &declared.name)
        .expect("the settings page renders a tile for the declared section");

    // The press: custom id to action to message to the navigation the session
    // exits with — the same chain the Host event loop walks.
    let action = registry
        .get(&tile)
        .expect("the rendered tile resolves to a registered action");
    let msg = SettingsFeature::translate(action, SelectValues::String(Vec::new()), &model)
        .expect("a section tile translates to its message");
    let Some(Navigation::SettingsSection { plugin, command }) =
        SettingsFeature::exit_navigation(&msg)
    else {
        panic!("a section click must exit into the settings section handoff, got {msg:?}");
    };
    assert_eq!(
        plugin, plugin_name,
        "the tile dispatches into the plugin whose manifest declared the section"
    );
    assert_eq!(
        command, declared.command,
        "the tile dispatches the panel command the manifest declared, not a \
         host-side guess at one"
    );

    // The handoff, in the order the Router performs it: park the settings
    // waiter on the live message, then adopt that message into the panel.
    returns.wait(serenity::MessageId::new(message_id));
    let mut message = serenity::Message::default();
    message.channel_id = serenity::ChannelId::new(channel_id).into();
    message.id = serenity::MessageId::new(message_id);
    adopt_message_into_section(
        &engine,
        panel.clone(),
        &command,
        json!({
            "guild_id": guild_id,
            "_context": admin_args(guild_id)["_context"],
        }),
        io.as_ref(),
        &message,
    )
    .await
    .expect("the settings tile hands the message to the plugin's panel");

    // The plugin's own render, delivered to Discord. A manifest that validates
    // while dispatching to nothing leaves this absent: no panel, no edit.
    let body = edited
        .lock()
        .expect("edit body lock")
        .take()
        .expect("the handoff edits the message into the panel");
    assert!(
        body.to_string().contains("Voice Tracking Settings"),
        "the message now carries the panel the voice plugin rendered: {body}"
    );
    assert!(
        engine.has_session(message.id).await,
        "the morphed message carries a plugin session, so the message's next \
         click lands in the panel the invoke opened"
    );

    // The falsifier: the plugin is not answering `/settings` with something
    // generic, it is matching on the command the invoke carried. A command the
    // manifest does not declare is refused, and the refusal quotes the command
    // the plugin received — so the panel above proves the declared command, not
    // merely that some invoke arrived.
    let rejected = engine
        .invoke(
            panel.clone(),
            "voice-settings-undeclared",
            json!({
                "guild_id": guild_id,
                "_context": admin_args(guild_id)["_context"],
            }),
        )
        .await;
    assert!(
        matches!(
            &rejected,
            Err(InteractionError::PluginRejected { msg, .. })
                if msg.contains("for command `voice-settings-undeclared`")
        ),
        "the plugin matches the invoke's command and names what it received, \
         so the panel above can only be the declared one: {rejected:?}"
    );
    panel.stop().await.expect("stop voice plugin");
}

/// The `custom_id` of the first button in a rendered component tree labelled
/// `label`. Walks the tree the way Discord does, so the test presses a tile the
/// page really rendered rather than one the registry would have minted.
fn find_button_id(page: &Value, label: &str) -> Option<String> {
    fn walk(node: &Value, label: &str) -> Option<String> {
        match node {
            Value::Object(map) => {
                if map.get("label").and_then(Value::as_str) == Some(label)
                    && let Some(id) = map.get("custom_id").and_then(Value::as_str)
                {
                    return Some(id.to_string());
                }
                map.values().find_map(|value| walk(value, label))
            }
            Value::Array(items) => items.iter().find_map(|item| walk(item, label)),
            _ => None,
        }
    }
    walk(page, label)
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
