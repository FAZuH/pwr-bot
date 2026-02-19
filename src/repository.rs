//! Database module with SQLite storage and SQLx.

use std::str::FromStr;

use log::debug;
use log::info;
use sqlx::SqlitePool;
use sqlx::sqlite::SqliteConnectOptions;

use crate::repository::table::FeedItemTable;
use crate::repository::table::FeedSubscriptionTable;
use crate::repository::table::FeedTable;
use crate::repository::table::ServerSettingsTable;
use crate::repository::table::SubscriberTable;
use crate::repository::table::TableBase;
use crate::repository::table::VoiceSessionsTable;

pub mod error;
pub mod table;

/// Main database struct containing all table handlers.
pub struct Repository {
    pool: SqlitePool,
    pub feed: FeedTable,
    pub feed_item: FeedItemTable,
    pub subscriber: SubscriberTable,
    pub feed_subscription: FeedSubscriptionTable,
    pub server_settings: ServerSettingsTable,
    pub voice_sessions: VoiceSessionsTable,
}

impl Repository {
    /// Creates a new database connection and initializes table handlers.
    pub async fn new(db_url: &str, db_path: &str) -> anyhow::Result<Self> {
        let path = std::path::Path::new(db_path);
        if !path.exists() {
            debug!("Database path {db_path} does not exist. Creating...");
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(path, "")?;
            info!("Created {db_path}");
        }

        debug!("Connecting to db...");
        let opts = SqliteConnectOptions::from_str(db_url)?.foreign_keys(true);
        let pool = SqlitePool::connect_with(opts).await?;
        log::log!(log::Level::Info, "Connected to db.");

        let feed = FeedTable::new(pool.clone());
        let feed_item = FeedItemTable::new(pool.clone());
        let subscriber = SubscriberTable::new(pool.clone());
        let feed_subscription = FeedSubscriptionTable::new(pool.clone());
        let server_settings = ServerSettingsTable::new(pool.clone());
        let voice_sessions = VoiceSessionsTable::new(pool.clone());

        Ok(Self {
            pool,
            feed,
            feed_item,
            subscriber,
            feed_subscription,
            server_settings,
            voice_sessions,
        })
    }

    /// Runs database migrations from the migrations directory.
    pub async fn run_migrations(&self) -> anyhow::Result<()> {
        sqlx::migrate!("./migrations").run(&self.pool).await?;
        Ok(())
    }

    /// Drops all tables. Use with caution!
    pub async fn drop_all_tables(&self) -> anyhow::Result<()> {
        self.feed.drop_table().await?;
        self.feed_item.drop_table().await?;
        self.subscriber.drop_table().await?;
        self.feed_subscription.drop_table().await?;
        self.server_settings.drop_table().await?;
        self.voice_sessions.drop_table().await?;
        Ok(())
    }

    /// Deletes all data from all tables. Use with caution!
    pub async fn delete_all_tables(&self) -> anyhow::Result<()> {
        self.feed.delete_all().await?;
        self.feed_item.delete_all().await?;
        self.subscriber.delete_all().await?;
        self.feed_subscription.delete_all().await?;
        self.server_settings.delete_all().await?;
        self.voice_sessions.delete_all().await?;
        Ok(())
    }
}
