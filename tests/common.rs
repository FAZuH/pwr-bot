//! Common database test utilities.

use std::sync::Arc;

use pwr_bot::repo::PgRepos;

#[path = "support/db.rs"]
pub mod db;

/// Sets up a test database connection to PostgreSQL.
///
/// `db::db_url()` hands out a fresh, empty database dedicated to this test
/// process (external server) or an embedded cluster's database, so the
/// migrations below run on a virgin schema: first pass, errors propagate.
pub async fn setup_db() -> Arc<PgRepos> {
    let db_url = db::db_url().await;

    let db = PgRepos::new(&db_url)
        .await
        .expect("Failed to connect to database");

    db.run_migrations()
        .await
        .expect("Failed to run migrations on the fresh per-process test database");
    db.delete_all_tables()
        .await
        .expect("Failed to clean database");

    Arc::new(db)
}

/// Cleans up the test database by deleting all data.
pub async fn teardown_db(db: &PgRepos) {
    db.delete_all_tables()
        .await
        .expect("Failed to clean database");
}
