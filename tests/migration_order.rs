//! The core migration source is idempotent and owns only core tables.
//!
//! This file holds the host's half of the shared-ledger contract and nothing
//! else: core declares exactly one version, creates exactly its own tables,
//! applies nothing to a database that already records it, and appends to a
//! ledger holding rows it does not own. The workspace-wide version-uniqueness
//! property lives in `tests/migration_versions.rs`, and each plugin crate
//! asserts its own ownership and ledger survival in its own tests. No source's
//! relative startup order is asserted anywhere: the ledger is keyed by version,
//! so order between sources is decided by the timestamps they carry and is not
//! an invariant.

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

/// Names the embedded source to the generic `MigrationHarness` method, which
/// reports the migrations it applied. `Connection`'s inherent method of the
/// same name returns `()`, so the source is handed over as a trait object to
/// reach the counting one.
struct CoreMigrations;

impl MigrationSource<Pg> for CoreMigrations {
    fn migrations(&self) -> diesel::migration::Result<Vec<Box<dyn Migration<Pg>>>> {
        <EmbeddedMigrations as MigrationSource<Pg>>::migrations(&CORE_MIGRATIONS)
    }
}

/// The version the core source's single migration carries.
const CORE_STORAGE_VERSION: &str = "202609250000000000";
/// A retired monolith-era version that must never reappear in the ledger.
const HISTORICAL_CORE_VERSION: &str = "202604291222520000";
/// A version no source declares, used to prove the ledger is not rewritten.
const ABSENT_VERSION: &str = "29990101000000";

/// Drops every table this and any sibling source could have created, so each
/// assertion below starts from a known-empty schema.
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

fn run_core_migrations(db_url: &str) -> usize {
    let mut connection = diesel::PgConnection::establish(db_url).expect("connect Diesel migration");
    connection
        .run_pending_migrations(CoreMigrations)
        .expect("run core migration source")
        .len()
}

fn embedded_versions() -> Vec<String> {
    <EmbeddedMigrations as MigrationSource<Pg>>::migrations(&CORE_MIGRATIONS)
        .expect("core migrations are readable")
        .into_iter()
        .map(|migration: Box<dyn Migration<Pg>>| migration.name().version().to_string())
        .collect()
}

/// Creates the two-column ledger diesel's migration harness writes, seeded
/// with `versions`, so a pre-existing ledger can be replayed.
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

/// The core source owns exactly one migration, and its version is the one it
/// declares — the host half of the uniqueness invariant its plugins each
/// assert for themselves.
#[test]
fn core_owns_exactly_one_migration_version() {
    assert_eq!(embedded_versions(), vec![CORE_STORAGE_VERSION.to_owned()]);
}

/// A fresh database gets core's four tables and one ledger entry, and a
/// repeated run applies nothing.
#[tokio::test]
#[serial_test::serial]
async fn core_migrations_are_idempotent_on_a_fresh_database() {
    let db_url = db::db_url().await;
    reset_database(&db_url).await;

    let url = db_url.clone();
    tokio::task::spawn_blocking(move || run_core_migrations(&url))
        .await
        .expect("core migration task");
    assert_tables_exist(
        &db_url,
        &["server_settings", "bot_meta", "plugin_kv", "guild_plugins"],
    )
    .await;
    let mut first = ledger_versions(&db_url).await;
    first.sort();
    assert_eq!(first, [CORE_STORAGE_VERSION]);
    assert!(
        !first.contains(&HISTORICAL_CORE_VERSION.to_string()),
        "a retired monolith-era version never reappears"
    );

    let reapplied = tokio::task::spawn_blocking({
        let url = db_url.clone();
        move || run_core_migrations(&url)
    })
    .await
    .expect("repeat core migration task");
    assert_eq!(reapplied, 0, "an already-migrated database applies nothing");
    assert_eq!(ledger_versions(&db_url).await.len(), 1);
}

/// A ledger that already records a retired or unrelated version keeps those
/// rows: the harness appends, it never rewrites the shared ledger.
#[tokio::test]
#[serial_test::serial]
async fn an_existing_shared_ledger_is_preserved() {
    let db_url = db::db_url().await;
    reset_database(&db_url).await;
    let url = db_url.clone();
    tokio::task::spawn_blocking(move || {
        seed_ledger(&url, &[HISTORICAL_CORE_VERSION, ABSENT_VERSION]);
        run_core_migrations(&url);
    })
    .await
    .expect("legacy ledger migration task");

    let mut versions = ledger_versions(&db_url).await;
    versions.sort();
    let mut expected = vec![
        ABSENT_VERSION.to_string(),
        CORE_STORAGE_VERSION.to_string(),
        HISTORICAL_CORE_VERSION.to_string(),
    ];
    expected.sort();
    assert_eq!(
        versions, expected,
        "existing ledger rows survive; core's migration is appended"
    );
}
