use async_trait::async_trait;
use sqlx::SqlitePool;

use super::{base_table::BaseTable, table::Table};
use crate::database::model::subscribers_model::SubsribersModel;

pub struct SubscribersTable {
    base: BaseTable,
}

impl SubscribersTable {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            base: BaseTable::new(pool),
        }
    }
}

impl SubscribersTable {
    pub async fn select_all_by_type(&self, r#type: &str) -> anyhow::Result<Vec<SubsribersModel>> {
        let ret = sqlx::query_as::<_, SubsribersModel>("SELECT id, subscriber_type, subscriber_id FROM subscribers WHERE subscriber_type = ?")
        .bind(r#type)
        .fetch_all(&self.base.pool)
        .await?;
        Ok(ret)
    }
}

#[async_trait]
impl Table<SubsribersModel, u32> for SubscribersTable {
    async fn create_table(&self) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS subscribers (
                id INTEGER PRIMARY KEY,
                subscriber_type TEXT NOT NULL,
                subscriber_id TEXT NOT NULL
            )
            "#,
        )
        .execute(&self.base.pool)
        .await?;
        Ok(())
    }

    async fn drop_table(&self) -> anyhow::Result<()> {
        sqlx::query("DROP TABLE IF EXISTS subscribers")
            .execute(&self.base.pool)
            .await?;
        Ok(())
    }

    async fn select_all(&self) -> anyhow::Result<Vec<SubsribersModel>> {
        let ret = sqlx::query_as::<_, SubsribersModel>("SELECT id, subscriber_type, subscriber_id FROM subscribers")
        .fetch_all(&self.base.pool)
        .await?;
        Ok(ret)
    }

    async fn delete_all(&self) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM subscribers")
            .execute(&self.base.pool)
            .await?;
        Ok(())
    }

    async fn select(&self, id: &u32) -> anyhow::Result<SubsribersModel> {
        let model = sqlx::query_as::<_, SubsribersModel>(
            "SELECT id, subscriber_type, subscriber_id FROM subscribers WHERE id = ?",
        )
        .bind(id)
        .fetch_one(&self.base.pool)
        .await?;
        Ok(model)
    }

    async fn insert(&self, model: &SubsribersModel) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO subscribers (subscriber_type, subscriber_id) VALUES (?, ?)",
        )
        .bind(&model.subscriber_type)
        .bind(&model.subscriber_id)
        .execute(&self.base.pool)
        .await?;
        Ok(())
    }

    async fn update(&self, model: &SubsribersModel) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE subscribers SET subscriber_type = ?, subscriber_id = ? WHERE id = ?",
        )
        .bind(&model.subscriber_type)
        .bind(&model.subscriber_id)
        .bind(model.id)
        .execute(&self.base.pool)
        .await?;
        Ok(())
    }

    async fn delete(&self, id: &u32) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM subscribers WHERE id = ?")
            .bind(id)
            .execute(&self.base.pool)
            .await?;
        Ok(())
    }
}
