use async_trait::async_trait;
use sqlx::Error as DbError;
use sqlx::SqlitePool;

use super::BaseTable;
use super::Table;
use crate::database::model::subscribers_model::SubscribersModel;

pub struct SubscribersTable {
    base: BaseTable,
}

impl SubscribersTable {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            base: BaseTable::new(pool),
        }
    }

    pub async fn select_all_by_type(&self, r#type: &str) -> Result<Vec<SubscribersModel>, DbError> {
        let ret = sqlx::query_as::<_, SubscribersModel>(
            "SELECT * FROM subscribers WHERE subscriber_type = ?",
        )
        .bind(r#type)
        .fetch_all(&self.base.pool)
        .await?;
        Ok(ret)
    }

    pub async fn select_all_by_latest_results(
        &self,
        latest_results_id: u32,
    ) -> Result<Vec<SubscribersModel>, DbError> {
        let ret = sqlx::query_as::<_, SubscribersModel>(
            "SELECT * FROM subscribers WHERE latest_results_id = ?",
        )
        .bind(latest_results_id)
        .fetch_all(&self.base.pool)
        .await?;
        Ok(ret)
    }

    pub async fn select_all_by_type_and_latest_results(
        &self,
        subscriber_type: &str,
        latest_results_id: u32,
    ) -> Result<Vec<SubscribersModel>, DbError> {
        let ret = sqlx::query_as::<_, SubscribersModel>(
            "SELECT * FROM subscribers WHERE subscriber_type = ? AND latest_results_id = ?",
        )
        .bind(subscriber_type)
        .bind(latest_results_id)
        .fetch_all(&self.base.pool)
        .await?;
        Ok(ret)
    }

    pub async fn delete_by_model(&self, model: SubscribersModel) -> Result<bool, DbError> {
        let res = sqlx::query(
            r#"
            DELETE FROM subscribers WHERE 
                subscriber_type = ? AND 
                subscriber_id = ? AND
                latest_results_id = ?
            "#,
        )
        .bind(model.subscriber_type)
        .bind(model.subscriber_id)
        .bind(model.latest_results_id)
        .execute(&self.base.pool)
        .await?;
        Ok(res.rows_affected() > 0)
    }

    pub async fn select_all_by_subscriber_id(
        &self,
        subscriber_id: &str,
    ) -> Result<Vec<SubscribersModel>, DbError> {
        let ret = sqlx::query_as::<_, SubscribersModel>(
            "SELECT * FROM subscribers WHERE subscriber_id = ?",
        )
        .bind(subscriber_id)
        .fetch_all(&self.base.pool)
        .await?;
        Ok(ret)
    }
}

#[async_trait]
impl Table<SubscribersModel, u32> for SubscribersTable {
    async fn create_table(&self) -> Result<(), DbError> {
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS subscribers (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                subscriber_type TEXT NOT NULL,
                subscriber_id TEXT NOT NULL,
                latest_results_id INTEGER,
                UNIQUE(subscriber_type, subscriber_id, latest_results_id),
                FOREIGN KEY (latest_results_id) REFERENCES latest_results(id)
                    ON DELETE CASCADE
                    ON UPDATE CASCADE
            )"#,
        )
        .execute(&self.base.pool)
        .await?;
        Ok(())
    }

    async fn drop_table(&self) -> Result<(), DbError> {
        sqlx::query("DROP TABLE IF EXISTS subscribers")
            .execute(&self.base.pool)
            .await?;
        Ok(())
    }

    async fn select_all(&self) -> Result<Vec<SubscribersModel>, DbError> {
        let ret = sqlx::query_as::<_, SubscribersModel>("SELECT * FROM subscribers")
            .fetch_all(&self.base.pool)
            .await?;
        Ok(ret)
    }

    async fn delete_all(&self) -> Result<(), DbError> {
        sqlx::query("DELETE FROM subscribers")
            .execute(&self.base.pool)
            .await?;
        Ok(())
    }

    async fn select(&self, id: &u32) -> Result<SubscribersModel, DbError> {
        let model = sqlx::query_as::<_, SubscribersModel>("SELECT * FROM subscribers WHERE id = ?")
            .bind(id)
            .fetch_one(&self.base.pool)
            .await?;
        Ok(model)
    }

    async fn insert(&self, model: &SubscribersModel) -> Result<u32, DbError> {
        let res = sqlx::query(
            r#"INSERT INTO subscribers
                    (subscriber_type, subscriber_id, latest_results_id)
                VALUES (?, ?, ?)"#,
        )
        .bind(&model.subscriber_type)
        .bind(&model.subscriber_id)
        .bind(&model.latest_results_id)
        .execute(&self.base.pool)
        .await?;
        // TODO: ID: i64 instead
        Ok(res
            .last_insert_rowid()
            .try_into()
            .expect("Failed to convert last_insert_rowid to u32"))
    }

    async fn update(&self, model: &SubscribersModel) -> Result<(), DbError> {
        sqlx::query("UPDATE subscribers SET subscriber_type = ?, subscriber_id = ?, latest_results_id = ? WHERE id = ?")
            .bind(&model.subscriber_type)
            .bind(&model.subscriber_id)
            .bind(&model.latest_results_id)
            .bind(model.id)
            .execute(&self.base.pool)
            .await?;
        Ok(())
    }

    async fn delete(&self, id: &u32) -> Result<(), DbError> {
        sqlx::query("DELETE FROM subscribers WHERE id = ?")
            .bind(id)
            .execute(&self.base.pool)
            .await?;
        Ok(())
    }
}
