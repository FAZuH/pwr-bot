use sqlx::SqlitePool;

use crate::database::table::{
    latest_updates_table::LatestUpdatesTable, subscribers_table::SubscribersTable, table::Table,
};

pub struct Database {
    pub pool: SqlitePool,
    pub latest_updates_table: LatestUpdatesTable,
    pub subscribers_table: SubscribersTable,
}

impl Database {
    pub async fn new(db_url: &str, db_path: &str) -> anyhow::Result<Self> {
        if !std::fs::metadata(db_path).is_ok() {
            std::fs::write(db_path, "")?;
        }

        let pool = SqlitePool::connect(db_url).await?;

        let latest_updates_table = LatestUpdatesTable::new(pool.clone());
        let subscribers_table = SubscribersTable::new(pool.clone());
        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&pool)
            .await?;

        Ok(Self {
            pool,
            latest_updates_table,
            subscribers_table,
        })
    }

    pub async fn create_all_tables(&self) -> anyhow::Result<()> {
        self.latest_updates_table.create_table().await?;
        self.subscribers_table.create_table().await?;
        Ok(())
    }

    pub async fn drop_all_tables(&self) -> anyhow::Result<()> {
        self.latest_updates_table.drop_table().await?;
        self.subscribers_table.drop_table().await?;
        Ok(())
    }

    pub async fn delete_all_tables(&self) -> anyhow::Result<()> {
        self.latest_updates_table.delete_all().await?;
        self.subscribers_table.delete_all().await?;
        Ok(())
    }
}
