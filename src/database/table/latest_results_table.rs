use async_trait::async_trait;
use sqlx::Error as DbError;
use sqlx::SqlitePool;

use super::BaseTable;
use super::Table;
use crate::database::model::latest_results_model::LatestResultModel;

pub struct LatestResultsTable {
    base: BaseTable,
}

impl LatestResultsTable {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            base: BaseTable::new(pool),
        }
    }

    pub async fn select_all_by_tag(
        &self,
        tag: &str, // e.g., "series"
    ) -> Result<Vec<LatestResultModel>, DbError> {
        let ret = sqlx::query_as::<_, LatestResultModel>(
            "SELECT * FROM latest_results WHERE tags LIKE ?",
        )
        .bind(format!("%{}%", tag))
        .fetch_all(&self.base.pool)
        .await?;
        Ok(ret)
    }

    pub async fn select_all_by_url_contains(
        &self,
        pattern: &str, // e.g., "mangadex.org"
    ) -> Result<Vec<LatestResultModel>, DbError> {
        let ret =
            sqlx::query_as::<_, LatestResultModel>("SELECT * FROM latest_results WHERE url LIKE ?")
                .bind(format!("%{}%", pattern))
                .fetch_all(&self.base.pool)
                .await?;
        Ok(ret)
    }

    pub async fn delete_all_by_url_contains(
        &self,
        pattern: &str, // e.g., "mangadex.org"
    ) -> Result<(), DbError> {
        sqlx::query("DELETE FROM latest_results WHERE url LIKE ?")
            .bind(format!("%{}%", pattern))
            .execute(&self.base.pool)
            .await?;
        Ok(())
    }

    pub async fn select_by_url(&self, url: &str) -> Result<LatestResultModel, DbError> {
        let res =
            sqlx::query_as::<_, LatestResultModel>("SELECT * FROM latest_results WHERE url = ?")
                .bind(url)
                .fetch_one(&self.base.pool)
                .await?;
        Ok(res)
    }
}

#[async_trait]
impl Table<LatestResultModel, u32> for LatestResultsTable {
    async fn create_table(&self) -> Result<(), DbError> {
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS latest_results (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                latest TEXT NOT NULL,
                tags TEXT DEFAULT NULL,
                published TIMESTAMP NOT NULL,
                url TEXT NOT NULL,
                UNIQUE(url)
            )"#,
        )
        .execute(&self.base.pool)
        .await?;
        Ok(())
    }

    async fn drop_table(&self) -> Result<(), DbError> {
        sqlx::query("DROP TABLE IF EXISTS latest_results")
            .execute(&self.base.pool)
            .await?;
        Ok(())
    }

    async fn select_all(&self) -> Result<Vec<LatestResultModel>, DbError> {
        let ret = sqlx::query_as::<_, LatestResultModel>("SELECT * FROM latest_results")
            .fetch_all(&self.base.pool)
            .await?;
        Ok(ret)
    }

    async fn delete_all(&self) -> Result<(), DbError> {
        sqlx::query("DELETE FROM latest_results")
            .execute(&self.base.pool)
            .await?;
        Ok(())
    }

    async fn select(&self, id: &u32) -> Result<LatestResultModel, DbError> {
        let model =
            sqlx::query_as::<_, LatestResultModel>("SELECT * FROM latest_results WHERE id = ?")
                .bind(id)
                .fetch_one(&self.base.pool)
                .await?;
        Ok(model)
    }

    async fn insert(&self, model: &LatestResultModel) -> Result<u32, DbError> {
        let res = sqlx::query(
            r#"
            INSERT INTO latest_results
                (name, latest, tags, published, url) 
            VALUES (?, ?, ?, ?, ?)"#,
        )
        .bind(&model.name)
        .bind(&model.latest)
        .bind(&model.tags)
        .bind(model.published)
        .bind(&model.url)
        .execute(&self.base.pool)
        .await?;
        Ok(res
            .last_insert_rowid()
            .try_into()
            .expect("Failed to convert last_insert_rowid to u32"))
    }

    async fn update(&self, model: &LatestResultModel) -> Result<(), DbError> {
        sqlx::query(
            r#"UPDATE latest_results 
            SET name = ?, latest = ?, tags = ?, published = ?, url = ?
            WHERE id = ?"#,
        )
        .bind(&model.name)
        .bind(&model.latest)
        .bind(&model.tags)
        .bind(model.published)
        .bind(&model.url)
        .bind(model.id)
        .execute(&self.base.pool)
        .await?;
        Ok(())
    }

    async fn delete(&self, id: &u32) -> Result<(), DbError> {
        sqlx::query("DELETE FROM latest_results WHERE id = ?")
            .bind(id)
            .execute(&self.base.pool)
            .await?;
        Ok(())
    }
}
