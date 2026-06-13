use std::sync::Arc;

use pwr_bot::repo::PgRepos;
use pwr_bot::repo::traits::*;

/// Sets up a test database connection to PostgreSQL.
pub async fn setup_db() -> Arc<PgRepos> {
    let db_url = std::env::var("DB_URL")
        .unwrap_or("postgres://pwr_bot:pwr_bot@localhost:5432/pwr_bot".to_string());

    let db = PgRepos::new(&db_url)
        .await
        .expect("Failed to connect to database");

    db.run_migrations().await.expect("Failed to run migrations");

    // Clean all test tables before each test run
    db.server_settings
        .delete_all()
        .await
        .expect("Failed to clean server_settings");
    db.bot_meta
        .delete_all()
        .await
        .expect("Failed to clean bot_meta");

    Arc::new(db)
}

/// Cleans up the test database by deleting all data.
pub async fn teardown_db(_db: &PgRepos) {}
