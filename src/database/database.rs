use std::str::FromStr;

use log::debug;
use log::info;
use sqlx::SqlitePool;
use sqlx::sqlite::SqliteConnectOptions;

use super::table::FeedItemTable;
use super::table::FeedSubscriptionTable;
use super::table::FeedTable;
use super::table::SubscriberTable;
use super::table::TableBase;

pub struct Database {
    pub pool: SqlitePool,
    pub feed_table: FeedTable,
    pub feed_item_table: FeedItemTable,
    pub subscriber_table: SubscriberTable,
    pub feed_subscription_table: FeedSubscriptionTable,
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
        info!("Connected to db.");

        let feed_table = FeedTable::new(pool.clone());
        let feed_item_table = FeedItemTable::new(pool.clone());
        let subscriber_table = SubscriberTable::new(pool.clone());
        let feed_subscription_table = FeedSubscriptionTable::new(pool.clone());

        Ok(Self {
            pool,
            feed_table,
            feed_item_table,
            subscriber_table,
            feed_subscription_table,
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
        Ok(())
    }

    pub async fn delete_all_tables(&self) -> anyhow::Result<()> {
        self.feed_table.delete_all().await?;
        self.feed_item_table.delete_all().await?;
        self.subscriber_table.delete_all().await?;
        self.feed_subscription_table.delete_all().await?;
        Ok(())
    }
}
