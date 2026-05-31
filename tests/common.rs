//! Common test utilities and mock implementations.

use std::sync::Arc;
use std::sync::RwLock;

use async_trait::async_trait;
use pwr_bot::feed::BasePlatform;
use pwr_bot::feed::FeedItem;
use pwr_bot::feed::FeedSource;
use pwr_bot::feed::Platform;
use pwr_bot::feed::PlatformInfo;
use pwr_bot::feed::error::FeedError;
use pwr_bot::repo::PgRepos;

/// Sets up a test database connection to PostgreSQL.
pub async fn setup_db() -> Arc<PgRepos> {
    let db_url = std::env::var("DB_URL")
        .unwrap_or("postgres://pwr_bot:pwr_bot@localhost:5432/pwr_bot".to_string());

    let db = PgRepos::new(&db_url)
        .await
        .expect("Failed to connect to database");

    db.delete_all_tables()
        .await
        .expect("Failed to clean database");

    db.run_migrations().await.expect("Failed to run migrations");

    Arc::new(db)
}

/// Cleans up the test database by deleting all data.
pub async fn teardown_db(db: &PgRepos) {
    db.delete_all_tables()
        .await
        .expect("Failed to clean database");
}
