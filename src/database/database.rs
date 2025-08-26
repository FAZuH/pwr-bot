use std::str::FromStr;

use sqlx::SqlitePool;
use sqlx::sqlite::SqliteConnectOptions;

use super::table::Table;
use super::table::latest_results_table::LatestResultsTable;
use super::table::subscribers_table::SubscribersTable;

pub struct Database {
    pub pool: SqlitePool,
    pub latest_results_table: LatestResultsTable,
    pub subscribers_table: SubscribersTable,
}

impl Database {
    pub async fn new(db_url: &str, db_path: &str) -> anyhow::Result<Self> {
        let path = std::path::Path::new(db_path);
        if !path.exists() {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(path, "")?;
        }

        let opts = SqliteConnectOptions::from_str(db_url)?.foreign_keys(true);
        let pool = SqlitePool::connect_with(opts).await?;

        let latest_updates_table = LatestResultsTable::new(pool.clone());
        let subscribers_table = SubscribersTable::new(pool.clone());

        Ok(Self {
            pool,
            latest_results_table: latest_updates_table,
            subscribers_table,
        })
    }

    pub async fn create_all_tables(&self) -> anyhow::Result<()> {
        sqlx::migrate!("./migrations").run(&self.pool).await?;
        Ok(())
    }

    pub async fn drop_all_tables(&self) -> anyhow::Result<()> {
        self.latest_results_table.drop_table().await?;
        self.subscribers_table.drop_table().await?;
        Ok(())
    }

    pub async fn delete_all_tables(&self) -> anyhow::Result<()> {
        self.latest_results_table.delete_all().await?;
        self.subscribers_table.delete_all().await?;
        Ok(())
    }
}
