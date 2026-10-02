//! This plugin's migration is safe to run against an existing database.
//!
//! A deployed instance already carries ledger rows. Running the source again
//! must APPEND, never rewrite: a lost ledger row means a source is re-applied
//! over live data, and a dropped row means a source never runs at all.

use diesel::Connection;
use diesel::connection::SimpleConnection;
use diesel::migration::Migration;
use diesel::migration::MigrationSource;
use diesel::pg::Pg;
use diesel_migrations::EmbeddedMigrations;
use diesel_migrations::MigrationHarness;
use diesel_migrations::embed_migrations;
use pwr_test_support::db;
use tokio_postgres::NoTls;

const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");

/// This plugin's ledger version — the same string `migration.rs` asserts its
/// directory name carries.
const VERSION: &str = "202609241501000000";
/// A retired version this plugin no longer declares, standing in for a row a
/// deployed database already holds from an earlier release.
const RETIRED_VERSION: &str = "202609241500000000";

/// Names the embedded source to the generic `MigrationHarness` method, which
/// reports the migrations it applied. `Connection`'s inherent method of the
/// same name returns `()`, so the source is handed over as a trait object.
struct Source;

impl MigrationSource<Pg> for Source {
    fn migrations(&self) -> diesel::migration::Result<Vec<Box<dyn Migration<Pg>>>> {
        <EmbeddedMigrations as MigrationSource<Pg>>::migrations(&MIGRATIONS)
    }
}

/// The tables this migration creates, dropped before each test so each starts
/// from a schema that owes nothing to the plugin.
fn owned_tables() -> [&'static str; 1] {
    ["welcome_settings"]
}

async fn reset_database(db_url: &str) {
    let (client, connection) = tokio_postgres::connect(db_url, NoTls)
        .await
        .expect("connect to migration test database");
    tokio::spawn(async move {
        connection.await.expect("migration test connection");
    });
    for table in owned_tables() {
        client
            .execute(
                format!("DROP TABLE IF EXISTS {table} CASCADE").as_str(),
                &[],
            )
            .await
            .expect("reset migration test database");
    }
    client
        .execute("DROP TABLE IF EXISTS __diesel_schema_migrations", &[])
        .await
        .expect("drop the migration ledger");
}

fn run_migrations(db_url: &str) -> usize {
    let mut connection = diesel::PgConnection::establish(db_url).expect("connect Diesel migration");
    connection
        .run_pending_migrations(Source)
        .expect("run the plugin migration source")
        .len()
}

/// Creates the two-column ledger diesel writes, seeded with `versions`.
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

async fn ledger_versions(db_url: &str) -> Vec<String> {
    let (client, connection) = tokio_postgres::connect(db_url, NoTls)
        .await
        .expect("connect migration ledger");
    tokio::spawn(async move {
        connection.await.expect("migration ledger connection");
    });
    client
        .query("SELECT version FROM __diesel_schema_migrations", &[])
        .await
        .expect("query migration ledger versions")
        .into_iter()
        .map(|row| row.get(0))
        .collect()
}

async fn assert_tables_exist(db_url: &str) {
    let (client, connection) = tokio_postgres::connect(db_url, NoTls)
        .await
        .expect("connect to migrated database");
    tokio::spawn(async move {
        connection.await.expect("migrated database connection");
    });
    for table in owned_tables() {
        let exists: bool = client
            .query_one(
                "SELECT EXISTS (SELECT 1 FROM information_schema.tables \
                 WHERE table_schema = 'public' AND table_name = $1)",
                &[&table],
            )
            .await
            .expect("query migrated table")
            .get(0);
        assert!(exists, "the migration did not create {table}");
    }
}

/// A fresh database gets this plugin's tables and exactly its own ledger row.
#[tokio::test]
#[serial_test::serial]
async fn a_fresh_database_gets_this_plugins_tables() {
    let db_url = db::db_url().await;
    reset_database(&db_url).await;

    let url = db_url.clone();
    let applied = tokio::task::spawn_blocking(move || run_migrations(&url))
        .await
        .expect("plugin migration task");

    assert_eq!(
        applied, 1,
        "a fresh database applies this source's migration"
    );
    assert_tables_exist(&db_url).await;
    assert_eq!(ledger_versions(&db_url).await, [VERSION.to_string()]);
}

/// The regression this guards: a deployed instance already holds a row for a
/// version this plugin no longer declares. Re-running the source must keep
/// that row — a harness that truncates or rebuilds the ledger would re-apply
/// migrations over live data.
#[tokio::test]
#[serial_test::serial]
async fn an_existing_ledger_row_from_an_earlier_release_survives() {
    let db_url = db::db_url().await;
    reset_database(&db_url).await;
    let url = db_url.clone();
    tokio::task::spawn_blocking(move || {
        seed_ledger(&url, &[RETIRED_VERSION]);
        run_migrations(&url);
    })
    .await
    .expect("legacy ledger migration task");

    assert_tables_exist(&db_url).await;
    let mut versions = ledger_versions(&db_url).await;
    versions.sort();
    assert_eq!(
        versions,
        [RETIRED_VERSION.to_string(), VERSION.to_string()],
        "the retired row survives beside the newly applied one"
    );
}

/// A database that already records THIS version is not re-applied, and keeps
/// the tables it has. This is the "already migrated" path every startup takes.
#[tokio::test]
#[serial_test::serial]
async fn an_already_migrated_database_applies_nothing() {
    let db_url = db::db_url().await;
    reset_database(&db_url).await;
    let url = db_url.clone();
    tokio::task::spawn_blocking(move || {
        run_migrations(&url);
        run_migrations(&url);
    })
    .await
    .expect("repeat migration task");

    assert_tables_exist(&db_url).await;
    assert_eq!(
        ledger_versions(&db_url).await,
        [VERSION.to_string()],
        "the ledger records this source once, however often it runs"
    );
}
