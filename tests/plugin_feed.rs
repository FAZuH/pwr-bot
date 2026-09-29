//! End-to-end tests for the feed plugin over the real stdio and database
//! seams.

use std::path::PathBuf;
use std::sync::Arc;

use feed::Platforms;
use feed::entity::FeedEntity;
use feed::entity::FeedSubscriptionEntity;
use feed::entity::SubscriberEntity;
use feed::entity::SubscriberType;
use feed::repo::Repository;
use feed::repo::traits::*;
use feed::service::feed_settings::FeedSettingsService;
use feed::service::feed_subscription::FeedSubscriptionService;
use feed::view::feed_batch::VIEW_SUBSCRIPTIONS;
use feed::view::feed_list::EDIT;
use pwr_bot::plugin::HostConfig;
use pwr_bot::plugin::HostServices;
use pwr_bot::plugin::InteractionEngine;
use pwr_bot::plugin::InteractionError;
use pwr_bot::plugin::PluginManager;
use pwr_bot::plugin::RespawnPolicy;
use pwr_bot::plugin::RunningPlugin;
use pwr_bot::plugin::StatsHandle;
use pwr_bot::plugin::host::MockHostIo;
use pwr_bot::repo::PgRepos;
use pwr_plugin_protocol::FeedsSettings;
use pwr_plugin_protocol::Msg;
use pwr_poise_components::IS_COMPONENTS_V2;
use serde_json::Value;
use serde_json::json;

#[path = "support/db.rs"]
mod db;
mod probe;
use probe::probe_binary;

const GUILD_ID: u64 = 42;

fn admin_context() -> Value {
    json!({
        "user_id": 7,
        "member_roles": [],
        "member_permissions": 1 << 5,
    })
}

fn sample_settings() -> FeedsSettings {
    FeedsSettings {
        enabled: Some(true),
        channel_id: Some("123456789".into()),
        subscribe_role_id: Some("987654321".into()),
        unsubscribe_role_id: None,
    }
}

async fn prepare_database() -> String {
    let db_url = db::db_url().await;
    let core = PgRepos::new(&db_url).await.expect("connect core storage");
    core.run_migrations().await.expect("run core migrations");
    let repository = Repository::connect(&db_url)
        .await
        .expect("connect feed storage");
    repository.migrate().await.expect("migrate feed storage");
    FeedSettingsService::new(repository)
        .update(GUILD_ID, sample_settings())
        .await
        .expect("seed feed settings");
    db_url
}

async fn install_settings_write_counter(db_url: &str) {
    let (client, connection) = tokio_postgres::connect(db_url, tokio_postgres::NoTls)
        .await
        .expect("connect write counter setup");
    tokio::spawn(async move {
        connection.await.expect("write counter setup connection");
    });
    client
        .batch_execute(
            "CREATE TABLE IF NOT EXISTS feed_settings_write_log (id BIGSERIAL PRIMARY KEY);
             CREATE OR REPLACE FUNCTION record_feed_settings_write() RETURNS trigger
             LANGUAGE plpgsql AS $$
             BEGIN
                 INSERT INTO feed_settings_write_log DEFAULT VALUES;
                 RETURN NEW;
             END;
             $$;
             DROP TRIGGER IF EXISTS feed_settings_write_count ON feed_settings;
             CREATE TRIGGER feed_settings_write_count
             AFTER INSERT OR UPDATE ON feed_settings
             FOR EACH ROW EXECUTE FUNCTION record_feed_settings_write();
             TRUNCATE feed_settings_write_log;",
        )
        .await
        .expect("install feed settings write counter");
}

async fn settings_write_count(db_url: &str) -> i64 {
    let (client, connection) = tokio_postgres::connect(db_url, tokio_postgres::NoTls)
        .await
        .expect("connect write counter query");
    tokio::spawn(async move {
        connection.await.expect("write counter query connection");
    });
    client
        .query_one("SELECT count(*) FROM feed_settings_write_log", &[])
        .await
        .expect("count feed settings writes")
        .get(0)
}

fn services(
    io: Arc<MockHostIo>,
    db_url: String,
    settings_returns: Option<Arc<pwr_bot::bot::translate::SettingsReturns>>,
) -> Arc<HostServices> {
    Arc::new(HostServices {
        io: Some(io),
        config: Some(HostConfig {
            db_url,
            data_path: PathBuf::from("/tmp/pwr-bot-test"),
            poll_interval: std::time::Duration::from_secs(30),
        }),
        kv: None,
        engine: None,
        stats: Arc::new(StatsHandle::default()),
        users: Default::default(),
        welcome: None,
        previews: None,
        settings_returns,
    })
}

async fn spawn_panel(services: Arc<HostServices>) -> Arc<RunningPlugin> {
    let manager =
        Arc::new(PluginManager::new(None, RespawnPolicy::default()).with_host_services(services));
    manager
        .spawn("feed", probe_binary("feed"), None, &[], &[])
        .await
        .expect("spawn feed plugin")
}

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

#[tokio::test]
#[serial_test::serial]
async fn core_and_plugin_migrations_create_owned_tables_once() {
    let db_url = db::db_url().await;
    let (client, connection) = tokio_postgres::connect(&db_url, tokio_postgres::NoTls)
        .await
        .expect("connect reset client");
    tokio::spawn(async move {
        connection.await.expect("reset connection");
    });
    client
        .batch_execute("DROP SCHEMA public CASCADE; CREATE SCHEMA public;")
        .await
        .expect("reset feed database");

    let core = PgRepos::new(&db_url).await.expect("connect core storage");
    core.run_migrations().await.expect("run core migrations");
    let repository = Repository::connect(&db_url)
        .await
        .expect("connect feed storage");

    let applied = repository.migrate().await.expect("migrate fresh database");
    assert!(
        !applied.is_empty(),
        "fresh database applies feed migrations"
    );

    let (client, connection) = tokio_postgres::connect(&db_url, tokio_postgres::NoTls)
        .await
        .expect("connect query client");
    tokio::spawn(async move {
        connection.await.expect("query connection");
    });
    for table in [
        "server_settings",
        "bot_meta",
        "plugin_kv",
        "guild_plugins",
        "feeds",
        "feed_items",
        "subscribers",
        "feed_subscriptions",
        "feed_settings",
    ] {
        let row = client
            .query_one("SELECT to_regclass($1)::text", &[&table])
            .await
            .expect("query created table");
        assert_eq!(row.get::<_, Option<String>>(0).as_deref(), Some(table));
    }

    core.run_migrations().await.expect("repeat core migrations");
    let reapplied = repository.migrate().await.expect("repeat feed migrations");
    assert!(
        reapplied.is_empty(),
        "an already-migrated database applies nothing"
    );
}

#[tokio::test]
#[serial_test::serial]
async fn open_edit_and_back_persist_the_snapshot_once_and_return() {
    let db_url = prepare_database().await;
    install_settings_write_counter(&db_url).await;
    let returns = Arc::new(pwr_bot::bot::translate::SettingsReturns::default());
    let panel = spawn_panel(services(
        Arc::new(MockHostIo::new()),
        db_url.clone(),
        Some(returns.clone()),
    ))
    .await;

    let resp = panel
        .call(
            "invoke",
            Some("feed-settings"),
            Some(json!({ "guild_id": GUILD_ID, "_context": admin_context() })),
        )
        .await
        .expect("panel invoke answered");
    let view = assert_envelope(&resp, 0);
    assert_eq!(view["guild_id"], json!(GUILD_ID));
    assert!(panel_status(&resp).contains("**active**"));

    let resp = panel
        .call(
            "view.interact",
            Some("feed-settings"),
            Some(json!({
                "custom_id": "feeds:toggle",
                "view": view,
                "_context": admin_context(),
            })),
        )
        .await
        .expect("toggle answered");
    let view = assert_envelope(&resp, 1);
    assert!(panel_status(&resp).contains("**paused**"));

    let rx = returns.wait(poise::serenity_prelude::MessageId::new(777));
    let resp = panel
        .call(
            "view.interact",
            Some("feed-settings"),
            Some(json!({
                "custom_id": "feeds:back",
                "view": view,
                "_context": admin_context(),
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

    let repository = Repository::connect(&db_url)
        .await
        .expect("reconnect feed storage");
    let persisted = FeedSettingsService::new(repository)
        .get(GUILD_ID)
        .await
        .expect("read persisted feed settings");
    assert_eq!(persisted.enabled, Some(false));
    assert_eq!(settings_write_count(&db_url).await, 1);
}

#[tokio::test]
#[serial_test::serial]
async fn expiry_persists_the_last_snapshot_once() {
    let db_url = prepare_database().await;
    install_settings_write_counter(&db_url).await;
    let panel = spawn_panel(services(Arc::new(MockHostIo::new()), db_url.clone(), None)).await;
    panel
        .call(
            "invoke",
            Some("feed-settings"),
            Some(json!({ "guild_id": GUILD_ID, "_context": admin_context() })),
        )
        .await
        .expect("feed panel startup");

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
    let status = panel.stop().await.expect("graceful stop");
    assert_eq!(status.code(), Some(0), "clean exit after bye: {status}");

    let repository = Repository::connect(&db_url)
        .await
        .expect("reconnect feed storage");
    let persisted = FeedSettingsService::new(repository)
        .get(GUILD_ID)
        .await
        .expect("read persisted feed settings");
    assert_eq!(persisted.enabled, Some(true));
    assert_eq!(settings_write_count(&db_url).await, 1);
}

#[tokio::test]
#[serial_test::serial]
async fn batch_can_open_and_edit_the_subscription_list() {
    let db_url = prepare_database().await;
    let repository = Repository::connect(&db_url)
        .await
        .expect("connect feed storage");
    repository.migrate().await.expect("migrate feed storage");
    let feed_id = repository
        .feed
        .insert(&FeedEntity {
            name: "Seeded feed".into(),
            platform_id: "anilist".into(),
            source_id: "seeded".into(),
            items_id: "seeded".into(),
            source_url: "https://anilist.co/anime/seeded".into(),
            ..FeedEntity::default()
        })
        .await
        .expect("insert feed");
    let subscriber_id = repository
        .subscriber
        .insert(&SubscriberEntity {
            r#type: SubscriberType::Dm,
            target_id: "777".into(),
            ..SubscriberEntity::default()
        })
        .await
        .expect("insert subscriber");
    repository
        .feed_subscription
        .insert(&FeedSubscriptionEntity {
            feed_id,
            subscriber_id,
            ..FeedSubscriptionEntity::default()
        })
        .await
        .expect("insert feed subscription");

    let panel = spawn_panel(services(Arc::new(MockHostIo::new()), db_url, None)).await;
    let actor = json!({
        "user_id": 777,
        "member_roles": [],
        "member_permissions": 0,
    });
    let (progress_tx, mut progress_rx) = tokio::sync::mpsc::unbounded_channel();
    let batch = panel
        .call_with_progress(
            "invoke",
            Some("feed unsubscribe"),
            Some(json!({
                "links": "https://unknown.example/feed",
                "_context": actor,
            })),
            progress_tx,
        )
        .await
        .expect("batch invoke answered");
    let progress = progress_rx.recv().await.expect("batch progress");
    assert_eq!(progress["view"]["model"]["phase"], json!("Confirm"));
    let batch_view = assert_envelope(&batch, 0);

    let list = panel
        .call(
            "view.interact",
            Some("feed unsubscribe"),
            Some(json!({
                "custom_id": VIEW_SUBSCRIPTIONS,
                "view": batch_view,
            })),
        )
        .await
        .expect("view subscriptions click answered");
    let list_view = assert_envelope(&list, 1);
    assert_eq!(
        list_view["model"]["subscriptions"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    let edit = panel
        .call(
            "view.interact",
            Some("feed unsubscribe"),
            Some(json!({
                "custom_id": EDIT,
                "view": list_view,
            })),
        )
        .await
        .expect("list edit click answered");
    let edit_view = assert_envelope(&edit, 2);
    assert_eq!(edit_view["model"]["state"], json!("Edit"));

    let status = panel.stop().await.expect("graceful stop");
    assert_eq!(status.code(), Some(0), "clean exit after bye: {status}");
}

#[tokio::test]
#[serial_test::serial]
async fn list_sessions_are_author_bound_end_to_end() {
    let db_url = prepare_database().await;
    let repository = Repository::connect(&db_url)
        .await
        .expect("connect feed storage");
    repository.migrate().await.expect("migrate feed storage");
    let feed_id = repository
        .feed
        .insert(&FeedEntity {
            name: "Author-only feed".into(),
            platform_id: "anilist".into(),
            source_id: "12345".into(),
            items_id: "12345".into(),
            source_url: "https://anilist.co/anime/12345".into(),
            ..FeedEntity::default()
        })
        .await
        .expect("insert feed");
    let user_id = 778;
    let subscriber_id = repository
        .subscriber
        .insert(&SubscriberEntity {
            r#type: SubscriberType::Dm,
            target_id: user_id.to_string(),
            ..SubscriberEntity::default()
        })
        .await
        .expect("insert subscriber");
    repository
        .feed_subscription
        .insert(&FeedSubscriptionEntity {
            feed_id,
            subscriber_id,
            ..FeedSubscriptionEntity::default()
        })
        .await
        .expect("insert feed subscription");
    let subscriber = repository
        .subscriber
        .select_by_type_and_target(&SubscriberType::Dm, &user_id.to_string())
        .await
        .expect("read subscriber")
        .expect("subscriber exists");
    let service = FeedSubscriptionService::new(&repository, Arc::new(Platforms::new()));
    let panel = spawn_panel(services(Arc::new(MockHostIo::new()), db_url, None)).await;
    let engine = InteractionEngine::new();
    let actor = json!({
        "user_id": user_id,
        "member_roles": [],
        "member_permissions": 0,
    });
    let spec = engine
        .invoke(
            panel.clone(),
            "feed list",
            json!({"sent_into": "DM", "_context": actor}),
        )
        .await
        .expect("invoke list");
    let message_id = poise::serenity_prelude::MessageId::new(991);
    engine
        .register(
            message_id,
            poise::serenity_prelude::UserId::new(user_id),
            panel.clone(),
            "feed list",
            spec,
        )
        .await;

    let before = service
        .list_paginated_subscriptions(&subscriber, 1_u32, 10_u32)
        .await
        .expect("read list before click");
    assert_eq!(before.len(), 1);
    let before_view = engine.view_state(message_id).await;
    for custom_id in [
        "feed-list:save",
        "feed-list:first",
        "feed-list:previous",
        "feed-list:next",
        "feed-list:last",
        "feed-list:unsub:0",
    ] {
        let error = engine
            .interact_validated(
                message_id,
                custom_id,
                json!({"user": {"id": 999}}),
                pwr_bot::plugin::validate_view_spec,
            )
            .await
            .expect_err("a different user cannot mutate the list");
        assert!(matches!(error, InteractionError::NotAuthor { .. }));
        assert_eq!(engine.view_state(message_id).await, before_view);
    }
    let after_rejected = service
        .list_paginated_subscriptions(&subscriber, 1_u32, 10_u32)
        .await
        .expect("read list after rejected click");
    assert_eq!(
        after_rejected.len(),
        1,
        "the rejected click performs no write"
    );

    let author_response = engine
        .interact_validated(
            message_id,
            "feed-list:unsub:0",
            json!({"user": {"id": user_id}}),
            pwr_bot::plugin::validate_view_spec,
        )
        .await
        .expect("the author can mutate the list");
    assert_eq!(
        author_response.view["model"]["marked_unsub"],
        json!(["https://anilist.co/anime/12345"])
    );
    let after_author = service
        .list_paginated_subscriptions(&subscriber, 1_u32, 10_u32)
        .await
        .expect("read list after author click");
    assert_eq!(
        after_author.len(),
        1,
        "a list toggle does not write a subscription"
    );

    let status = panel.stop().await.expect("graceful stop");
    assert_eq!(status.code(), Some(0), "clean exit after bye: {status}");
}

#[tokio::test]
#[serial_test::serial]
async fn feed_settings_rejects_an_invocation_without_actor_context() {
    let db_url = prepare_database().await;
    let panel = spawn_panel(services(Arc::new(MockHostIo::new()), db_url, None)).await;

    let resp = panel
        .call(
            "invoke",
            Some("feed settings"),
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
            assert_eq!(err.kind, "CommandError");
            assert!(err.msg.contains("actor context"), "msg: {}", err.msg);
        }
        other => panic!("expected a permission error, got {other:?}"),
    }

    let status = panel.stop().await.expect("graceful stop");
    assert_eq!(status.code(), Some(0), "clean exit after bye: {status}");
}
