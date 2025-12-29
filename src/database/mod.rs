use std::str::FromStr;

use log::debug;
use log::info;
use sqlx::SqlitePool;
use sqlx::sqlite::SqliteConnectOptions;

use crate::database::table::FeedItemTable;
use crate::database::table::FeedSubscriptionTable;
use crate::database::table::FeedTable;
use crate::database::table::ServerSettingsTable;
use crate::database::table::SubscriberTable;
use crate::database::table::TableBase;

pub mod error;
pub mod model;
pub mod table;

pub struct Database {
    pub pool: SqlitePool,
    pub feed_table: FeedTable,
    pub feed_item_table: FeedItemTable,
    pub subscriber_table: SubscriberTable,
    pub feed_subscription_table: FeedSubscriptionTable,
    pub server_settings_table: ServerSettingsTable,
}

impl Database {
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

        let feed_table = FeedTable::new(pool.clone());
        let feed_item_table = FeedItemTable::new(pool.clone());
        let subscriber_table = SubscriberTable::new(pool.clone());
        let feed_subscription_table = FeedSubscriptionTable::new(pool.clone());
        let server_settings_table = ServerSettingsTable::new(pool.clone());

        Ok(Self {
            pool,
            feed_table,
            feed_item_table,
            subscriber_table,
            feed_subscription_table,
            server_settings_table,
        })
    }

    pub async fn run_migrations(&self) -> anyhow::Result<()> {
        sqlx::migrate!("./migrations").run(&self.pool).await?;
        Ok(())
    }

    pub async fn drop_all_tables(&self) -> anyhow::Result<()> {
        self.feed_table.drop_table().await?;
        self.feed_item_table.drop_table().await?;
        self.subscriber_table.drop_table().await?;
        self.feed_subscription_table.drop_table().await?;
        self.server_settings_table.drop_table().await?;
        Ok(())
    }

    pub async fn delete_all_tables(&self) -> anyhow::Result<()> {
        self.feed_table.delete_all().await?;
        self.feed_item_table.delete_all().await?;
        self.subscriber_table.delete_all().await?;
        self.feed_subscription_table.delete_all().await?;
        self.server_settings_table.delete_all().await?;
        Ok(())
    }
}
