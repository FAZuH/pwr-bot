use async_trait::async_trait;
use sqlx::SqlitePool;

use super::base_table::BaseTable;
use super::table::Table;
use crate::database::model::latest_updates_model::LatestUpdatesModel;

pub struct LatestUpdatesTable {
    base: BaseTable,
}

impl LatestUpdatesTable {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            base: BaseTable::new(pool),
        }
    }

    pub async fn select_all_by_type(
        &self,
        r#type: &str,
    ) -> anyhow::Result<Vec<LatestUpdatesModel>> {
        let ret =
            sqlx::query_as::<_, LatestUpdatesModel>("SELECT * FROM latest_updates WHERE type = ?")
                .bind(r#type)
                .fetch_all(&self.base.pool)
                .await?;
        Ok(ret)
    }

    pub async fn select_by_model(
        &self,
        model: &LatestUpdatesModel,
    ) -> anyhow::Result<LatestUpdatesModel> {
        let res = sqlx::query_as::<_, LatestUpdatesModel>(
            "SELECT * FROM latest_updates WHERE type = ? AND series_id = ?",
        )
        .bind(&model.r#type)
        .bind(&model.series_id)
        .fetch_one(&self.base.pool)
        .await?;
        Ok(res)
    }
}

#[async_trait]
impl Table<LatestUpdatesModel, u32> for LatestUpdatesTable {
    async fn create_table(&self) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS latest_updates (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                type TEXT NOT NULL,
                series_id TEXT NOT NULL,
                series_latest TEXT NOT NULL,
                series_title TEXT NOT NULL,
                series_published TIMESTAMP NOT NULL,
                UNIQUE(type, series_id)
            )
            "#,
        )
        .execute(&self.base.pool)
        .await?;
        Ok(())
    }

    async fn drop_table(&self) -> anyhow::Result<()> {
        sqlx::query("DROP TABLE IF EXISTS latest_updates")
            .execute(&self.base.pool)
            .await?;
        Ok(())
    }

    async fn select_all(&self) -> anyhow::Result<Vec<LatestUpdatesModel>> {
        let ret = sqlx::query_as::<_, LatestUpdatesModel>(
            "SELECT * FROM latest_updates",
        )
        .fetch_all(&self.base.pool)
        .await?;
        Ok(ret)
    }

    async fn delete_all(&self) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM latest_updates")
            .execute(&self.base.pool)
            .await?;
        Ok(())
    }

    async fn select(&self, id: &u32) -> anyhow::Result<LatestUpdatesModel> {
        let model = sqlx::query_as::<_, LatestUpdatesModel>(
            "SELECT * FROM latest_updates WHERE id = ?",
        )
        .bind(id)
        .fetch_one(&self.base.pool)
        .await?;
        Ok(model)
    }

    async fn insert(&self, model: &LatestUpdatesModel) -> anyhow::Result<u32> {
        let res = sqlx::query(
            r#"
            INSERT INTO latest_updates
                (type, series_id, series_latest, series_title, series_published) 
            VALUES (?, ?, ?, ?, ?)"#
        )
        .bind(&model.r#type)
        .bind(&model.series_id)
        .bind(&model.series_latest)
        .bind(&model.series_title)
        .bind(model.series_published)
        .execute(&self.base.pool)
        .await?;
        Ok(res.last_insert_rowid().try_into()?)
    }

    async fn update(&self, model: &LatestUpdatesModel) -> anyhow::Result<()> {
        sqlx::query(
            r#"UPDATE latest_updates 
            SET type = ?,
                series_id = ?,
                series_latest = ?,
                series_title = ?,
                series_published = ?
            WHERE id = ?"#,
        )
        .bind(&model.r#type)
        .bind(&model.series_id)
        .bind(&model.series_latest)
        .bind(&model.series_title)
        .bind(model.series_published)
        .bind(model.id)
        .execute(&self.base.pool)
        .await?;
        Ok(())
    }

    async fn delete(&self, id: &u32) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM latest_updates WHERE id = ?")
            .bind(id)
            .execute(&self.base.pool)
            .await?;
        Ok(())
    }
}
