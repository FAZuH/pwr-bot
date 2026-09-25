use std::collections::BTreeSet;

use diesel::Connection;
use diesel::connection::SimpleConnection;
use diesel::migration::Migration;
use diesel::migration::MigrationSource;
use diesel::pg::Pg;
use diesel_migrations::EmbeddedMigrations;
use diesel_migrations::MigrationHarness;
use diesel_migrations::embed_migrations;
use tokio_postgres::NoTls;

#[path = "support/db.rs"]
mod db;

const CORE_MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");
const FEED_MIGRATIONS: EmbeddedMigrations = embed_migrations!("crates/plugin/feed/migrations");
const VOICE_MIGRATIONS: EmbeddedMigrations = embed_migrations!("crates/plugin/voice/migrations");

struct StaticMigrations(&'static EmbeddedMigrations);

impl MigrationSource<Pg> for StaticMigrations {
    fn migrations(&self) -> diesel::migration::Result<Vec<Box<dyn Migration<Pg>>>> {
        <EmbeddedMigrations as MigrationSource<Pg>>::migrations(self.0)
    }
}

fn embedded_versions(source: &'static EmbeddedMigrations) -> Vec<String> {
    <EmbeddedMigrations as MigrationSource<Pg>>::migrations(source)
        .expect("embedded migrations are readable")
        .into_iter()
        .map(|migration| migration.name().version().to_string())
        .collect()
}

const CORE_VERSION: &str = "202604291222520000";
const PHASE_FOUR_VERSION: &str = "202609241200000000";
const ABSENT_VERSION: &str = "29990101000000";
const CORE_STORAGE_VERSION: &str = "202609250000000000";
const FEED_PREVIOUS_VERSION: &str = "202609241300000000";
const FEED_OWNED_VERSION: &str = "202609241301000000";
const VOICE_PREVIOUS_VERSION: &str = "202609241400000000";
const VOICE_OWNED_VERSION: &str = "202609241401000000";

#[derive(Clone, Copy)]
enum Order {
    CoreFeedVoice,
    CoreVoiceFeed,
    FeedVoiceCore,
}

async fn reset_database(db_url: &str) {
    let (client, connection) = tokio_postgres::connect(db_url, NoTls)
        .await
        .expect("connect to migration test database");
    tokio::spawn(async move {
        connection.await.expect("migration test connection");
    });
    for table in [
        "guild_plugins",
        "plugin_kv",
        "bot_meta",
        "server_settings",
        "feed_settings",
        "feed_subscriptions",
        "feed_items",
        "subscribers",
        "feeds",
        "voice_settings_import_state",
        "voice_settings",
        "voice_sessions",
        "__diesel_schema_migrations",
    ] {
        client
            .execute(
                format!("DROP TABLE IF EXISTS {table} CASCADE").as_str(),
                &[],
            )
            .await
            .expect("reset migration test database");
    }
}

fn run_sources(db_url: &str, sources: &[&'static EmbeddedMigrations]) {
    let mut connection = diesel::PgConnection::establish(db_url).expect("connect Diesel migration");
    for &migrations in sources {
        connection
            .run_pending_migrations(StaticMigrations(migrations))
            .expect("run migration source");
    }
}

fn run_source(db_url: &str, source: &'static EmbeddedMigrations) {
    run_sources(db_url, &[source]);
}

fn run_order(db_url: &str, order: Order) {
    let sources: [&'static EmbeddedMigrations; 3] = match order {
        Order::CoreFeedVoice => [&CORE_MIGRATIONS, &FEED_MIGRATIONS, &VOICE_MIGRATIONS],
        Order::CoreVoiceFeed => [&CORE_MIGRATIONS, &VOICE_MIGRATIONS, &FEED_MIGRATIONS],
        Order::FeedVoiceCore => [&FEED_MIGRATIONS, &VOICE_MIGRATIONS, &CORE_MIGRATIONS],
    };
    run_sources(db_url, &sources);
}

fn seed_ledger(db_url: &str, versions: &[&str]) {
    let mut connection = diesel::PgConnection::establish(db_url).expect("connect legacy ledger");
    let values = versions
        .iter()
        .map(|version| format!("('{version}', CURRENT_TIMESTAMP)"))
        .collect::<Vec<_>>()
        .join(", ");
    connection
        .batch_execute(&format!(
            "CREATE TABLE __diesel_schema_migrations (\
             version VARCHAR(50) PRIMARY KEY, run_on TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP\
             );\
             INSERT INTO __diesel_schema_migrations (version, run_on) VALUES {values};"
        ))
        .expect("seed existing migration ledger");
}

/// Creates the seven-table schema and two-column ledger used by the monolith.
fn seed_legacy_core_schema(db_url: &str) {
    let mut connection = diesel::PgConnection::establish(db_url).expect("connect legacy schema");
    connection
        .batch_execute(
            "CREATE TABLE server_settings (guild_id BIGINT PRIMARY KEY, settings JSONB NOT NULL);\
             CREATE TABLE bot_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);\
             CREATE TABLE feeds (\
                 id SERIAL PRIMARY KEY, name TEXT NOT NULL, description TEXT NOT NULL DEFAULT '',\
                 platform_id TEXT NOT NULL, source_id TEXT NOT NULL, items_id TEXT NOT NULL,\
                 source_url TEXT NOT NULL, cover_url TEXT NOT NULL DEFAULT '', tags TEXT NOT NULL DEFAULT ''\
             );\
             CREATE TABLE feed_items (\
                 id SERIAL PRIMARY KEY, feed_id INTEGER NOT NULL, description TEXT NOT NULL,\
                 published TIMESTAMPTZ NOT NULL\
             );\
             CREATE TABLE subscribers (id SERIAL PRIMARY KEY, type TEXT NOT NULL, target_id TEXT NOT NULL);\
             CREATE TABLE feed_subscriptions (\
                 id SERIAL PRIMARY KEY, feed_id INTEGER NOT NULL, subscriber_id INTEGER NOT NULL\
             );\
             CREATE TABLE voice_sessions (\
                 id SERIAL PRIMARY KEY, user_id BIGINT NOT NULL, guild_id BIGINT NOT NULL,\
                 channel_id BIGINT NOT NULL, join_time TIMESTAMPTZ NOT NULL,\
                 leave_time TIMESTAMPTZ NOT NULL, is_active BOOLEAN NOT NULL DEFAULT FALSE\
             );\
             CREATE TABLE __diesel_schema_migrations (\
                 version VARCHAR(50) PRIMARY KEY, run_on TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP\
             );\
             INSERT INTO __diesel_schema_migrations (version, run_on)\
             VALUES ('202604291222520000', CURRENT_TIMESTAMP),\
                    ('202609241200000000', CURRENT_TIMESTAMP),\
                     ('29990101000000', CURRENT_TIMESTAMP);",
        )
        .expect("seed legacy core schema");
}

async fn assert_ledger_shape_and_versions(db_url: &str, expected_versions: &[&str]) {
    let (client, connection) = tokio_postgres::connect(db_url, NoTls)
        .await
        .expect("connect migration ledger");
    tokio::spawn(async move {
        connection.await.expect("migration ledger connection");
    });
    let columns = client
        .query(
            "SELECT column_name FROM information_schema.columns \
             WHERE table_schema = 'public' AND table_name = '__diesel_schema_migrations' \
             ORDER BY ordinal_position",
            &[],
        )
        .await
        .expect("query migration ledger columns")
        .into_iter()
        .map(|row| row.get::<_, String>(0))
        .collect::<Vec<_>>();
    assert_eq!(columns, ["version", "run_on"]);
    let rows = client
        .query("SELECT version FROM __diesel_schema_migrations", &[])
        .await
        .expect("query migration ledger versions");
    let versions: BTreeSet<String> = rows.into_iter().map(|row| row.get(0)).collect();
    let expected: BTreeSet<String> = expected_versions
        .iter()
        .map(|version| (*version).to_string())
        .collect();
    assert_eq!(versions, expected, "migration ledger contents differ");
}

async fn assert_ledger_does_not_contain(db_url: &str, forbidden_version: &str) {
    let (client, connection) = tokio_postgres::connect(db_url, NoTls)
        .await
        .expect("connect migration ledger");
    tokio::spawn(async move {
        connection.await.expect("migration ledger connection");
    });
    let present: bool = client
        .query_one(
            "SELECT EXISTS (SELECT 1 FROM __diesel_schema_migrations WHERE version = $1)",
            &[&forbidden_version],
        )
        .await
        .expect("query forbidden migration version")
        .get(0);
    assert!(!present, "migration source retained {forbidden_version}");
}

async fn assert_tables_exist(db_url: &str, tables: &[&str]) {
    let (client, connection) = tokio_postgres::connect(db_url, NoTls)
        .await
        .expect("connect to migrated database");
    tokio::spawn(async move {
        connection.await.expect("migrated database connection");
    });
    for table in tables {
        let exists: bool = client
            .query_one(
                "SELECT EXISTS (SELECT 1 FROM information_schema.tables \
                 WHERE table_schema = 'public' AND table_name = $1)",
                &[&table],
            )
            .await
            .expect("query migrated table")
            .get(0);
        assert!(exists, "missing table {table}");
    }
}

async fn assert_schema_and_versions_with_existing(db_url: &str, existing_versions: &[&str]) {
    assert_tables_exist(
        db_url,
        &[
            "server_settings",
            "bot_meta",
            "plugin_kv",
            "guild_plugins",
            "feeds",
            "feed_items",
            "subscribers",
            "feed_subscriptions",
            "feed_settings",
            "voice_sessions",
            "voice_settings",
            "voice_settings_import_state",
        ],
    )
    .await;
    let mut expected = existing_versions.to_vec();
    expected.extend([
        CORE_STORAGE_VERSION,
        FEED_OWNED_VERSION,
        VOICE_OWNED_VERSION,
    ]);
    assert_ledger_shape_and_versions(db_url, &expected).await;
}

async fn assert_schema_and_versions(db_url: &str) {
    assert_schema_and_versions_with_existing(db_url, &[]).await;
}

#[test]
fn embedded_owners_have_one_globally_unique_version_each() {
    let core = embedded_versions(&CORE_MIGRATIONS);
    let feed = embedded_versions(&FEED_MIGRATIONS);
    let voice = embedded_versions(&VOICE_MIGRATIONS);

    assert_eq!(core, vec![CORE_STORAGE_VERSION.to_owned()]);
    assert_eq!(feed, vec![FEED_OWNED_VERSION.to_owned()]);
    assert_eq!(voice, vec![VOICE_OWNED_VERSION.to_owned()]);

    let all = core
        .iter()
        .chain(&feed)
        .chain(&voice)
        .cloned()
        .collect::<BTreeSet<_>>();
    assert_eq!(all.len(), 3, "migration versions are globally unique");
}

#[tokio::test]
#[serial_test::serial]
async fn existing_feed_ledger_is_preserved_when_feed_migrations_run() {
    let db_url = db::db_url().await;
    reset_database(&db_url).await;
    let url = db_url.clone();
    tokio::task::spawn_blocking(move || {
        seed_ledger(&url, &[FEED_PREVIOUS_VERSION]);
        run_source(&url, &FEED_MIGRATIONS);
    })
    .await
    .expect("feed ledger migration task");

    assert_tables_exist(&db_url, &["feeds", "feed_settings"]).await;
    assert_ledger_shape_and_versions(&db_url, &[FEED_PREVIOUS_VERSION, FEED_OWNED_VERSION]).await;
}

#[tokio::test]
#[serial_test::serial]
async fn existing_voice_ledger_is_preserved_when_voice_migrations_run() {
    let db_url = db::db_url().await;
    reset_database(&db_url).await;
    let url = db_url.clone();
    tokio::task::spawn_blocking(move || {
        seed_ledger(&url, &[VOICE_PREVIOUS_VERSION]);
        run_source(&url, &VOICE_MIGRATIONS);
    })
    .await
    .expect("voice ledger migration task");

    assert_tables_exist(&db_url, &["voice_sessions", "voice_settings"]).await;
    assert_ledger_shape_and_versions(&db_url, &[VOICE_PREVIOUS_VERSION, VOICE_OWNED_VERSION]).await;
}

#[tokio::test]
#[serial_test::serial]
async fn existing_mixed_component_ledger_is_preserved_when_all_migrations_run() {
    let db_url = db::db_url().await;
    reset_database(&db_url).await;
    let url = db_url.clone();
    tokio::task::spawn_blocking(move || {
        seed_ledger(
            &url,
            &[
                FEED_PREVIOUS_VERSION,
                VOICE_PREVIOUS_VERSION,
                ABSENT_VERSION,
            ],
        );
        run_order(&url, Order::CoreFeedVoice);
    })
    .await
    .expect("mixed ledger migration task");

    assert_schema_and_versions_with_existing(
        &db_url,
        &[
            FEED_PREVIOUS_VERSION,
            VOICE_PREVIOUS_VERSION,
            ABSENT_VERSION,
        ],
    )
    .await;
}

#[tokio::test]
#[serial_test::serial]
async fn core_and_plugin_migrations_converge_in_every_startup_order() {
    let db_url = db::db_url().await;
    for order in [
        Order::CoreFeedVoice,
        Order::CoreVoiceFeed,
        Order::FeedVoiceCore,
    ] {
        reset_database(&db_url).await;
        let url = db_url.clone();
        tokio::task::spawn_blocking(move || run_order(&url, order))
            .await
            .expect("migration order task");
        assert_schema_and_versions(&db_url).await;
        assert_ledger_does_not_contain(&db_url, CORE_VERSION).await;
        assert_ledger_does_not_contain(&db_url, PHASE_FOUR_VERSION).await;
    }

    reset_database(&db_url).await;
    let url = db_url.clone();
    tokio::task::spawn_blocking(move || {
        seed_legacy_core_schema(&url);
        run_order(&url, Order::CoreFeedVoice);
    })
    .await
    .expect("legacy schema migration task");
    assert_schema_and_versions_with_existing(
        &db_url,
        &[CORE_VERSION, PHASE_FOUR_VERSION, ABSENT_VERSION],
    )
    .await;
}
