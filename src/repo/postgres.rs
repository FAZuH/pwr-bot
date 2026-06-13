use diesel::prelude::*;
use diesel_async::RunQueryDsl;

use crate::entity::*;
use crate::repo::DbPool;
use crate::repo::error::DatabaseError;
use crate::repo::schema::*;
use crate::repo::traits::*;

macro_rules! impl_table_base {
    ($struct_name:ident, $table:path) => {
        #[async_trait::async_trait]
        impl TableBase for $struct_name {
            async fn create_table(&self) -> Result<(), DatabaseError> {
                Ok(())
            }

            async fn drop_table(&self) -> Result<(), DatabaseError> {
                let mut conn = self.pool.get().await?;
                diesel::sql_query(concat!("DROP TABLE IF EXISTS ", stringify!($table)))
                    .execute(&mut conn)
                    .await?;
                Ok(())
            }

            async fn delete_all(&self) -> Result<(), DatabaseError> {
                let mut conn = self.pool.get().await?;
                diesel::delete($table).execute(&mut conn).await?;
                Ok(())
            }
        }
    };
}

// ============================================================================
// PgServerSettingsRepo
// ============================================================================

#[derive(Clone)]
pub struct PgServerSettingsRepo {
    pool: DbPool,
}

impl PgServerSettingsRepo {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }
}

impl_table_base!(PgServerSettingsRepo, server_settings::table);

#[async_trait::async_trait]
impl CrudTable<ServerSettingsEntity, u64> for PgServerSettingsRepo {
    async fn select_all(&self) -> Result<Vec<ServerSettingsEntity>, DatabaseError> {
        let mut conn = self.pool.get().await?;
        Ok(server_settings::table
            .select(ServerSettingsEntity::as_select())
            .load(&mut conn)
            .await?)
    }

    async fn insert(&self, model: &ServerSettingsEntity) -> Result<u64, DatabaseError> {
        let mut conn = self.pool.get().await?;
        let guild_id: DbU64 = diesel::insert_into(server_settings::table)
            .values(model)
            .returning(server_settings::guild_id)
            .get_result(&mut conn)
            .await?;
        Ok(guild_id.into())
    }

    async fn select(&self, id: &u64) -> Result<Option<ServerSettingsEntity>, DatabaseError> {
        let mut conn = self.pool.get().await?;
        Ok(server_settings::table
            .find(DbU64::from(*id))
            .select(ServerSettingsEntity::as_select())
            .first(&mut conn)
            .await
            .optional()?)
    }

    async fn update(&self, model: &ServerSettingsEntity) -> Result<(), DatabaseError> {
        let mut conn = self.pool.get().await?;
        diesel::update(server_settings::table.find(model.guild_id))
            .set(model)
            .execute(&mut conn)
            .await?;
        Ok(())
    }

    async fn delete(&self, id: &u64) -> Result<(), DatabaseError> {
        let mut conn = self.pool.get().await?;
        diesel::delete(server_settings::table.find(DbU64::from(*id)))
            .execute(&mut conn)
            .await?;
        Ok(())
    }

    async fn replace(&self, model: &ServerSettingsEntity) -> Result<u64, DatabaseError> {
        let gid: u64 = model.guild_id.into();
        if self.select(&gid).await?.is_some() {
            self.update(model).await?;
            return Ok(gid);
        }
        self.insert(model).await
    }
}

#[async_trait::async_trait]
impl ServerSettingsRepository for PgServerSettingsRepo {}

// ============================================================================
// PgBotMetaRepo
// ============================================================================

#[derive(Clone)]
pub struct PgBotMetaRepo {
    pool: DbPool,
}

impl PgBotMetaRepo {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }
}

impl_table_base!(PgBotMetaRepo, bot_meta::table);

#[async_trait::async_trait]
impl CrudTable<BotMetaEntity, String> for PgBotMetaRepo {
    async fn select_all(&self) -> Result<Vec<BotMetaEntity>, DatabaseError> {
        let mut conn = self.pool.get().await?;
        Ok(bot_meta::table
            .select(BotMetaEntity::as_select())
            .load(&mut conn)
            .await?)
    }

    async fn insert(&self, model: &BotMetaEntity) -> Result<String, DatabaseError> {
        let mut conn = self.pool.get().await?;
        let key = diesel::insert_into(bot_meta::table)
            .values(model)
            .returning(bot_meta::key)
            .get_result(&mut conn)
            .await?;
        Ok(key)
    }

    async fn select(&self, id: &String) -> Result<Option<BotMetaEntity>, DatabaseError> {
        let mut conn = self.pool.get().await?;
        Ok(bot_meta::table
            .find(id)
            .select(BotMetaEntity::as_select())
            .first(&mut conn)
            .await
            .optional()?)
    }

    async fn update(&self, model: &BotMetaEntity) -> Result<(), DatabaseError> {
        let mut conn = self.pool.get().await?;
        diesel::update(bot_meta::table.find(&model.key))
            .set(model)
            .execute(&mut conn)
            .await?;
        Ok(())
    }

    async fn delete(&self, id: &String) -> Result<(), DatabaseError> {
        let mut conn = self.pool.get().await?;
        diesel::delete(bot_meta::table.find(id))
            .execute(&mut conn)
            .await?;
        Ok(())
    }

    async fn replace(&self, model: &BotMetaEntity) -> Result<String, DatabaseError> {
        if self.select(&model.key).await?.is_some() {
            self.update(model).await?;
            return Ok(model.key.clone());
        }
        self.insert(model).await
    }
}

#[async_trait::async_trait]
impl BotMetaRepository for PgBotMetaRepo {
    async fn table_exists(&self) -> bool {
        let mut conn = match self.pool.get().await {
            Ok(c) => c,
            Err(_) => return false,
        };
        diesel::sql_query(
            "SELECT 1 FROM information_schema.tables WHERE table_schema = 'public' AND table_name = 'bot_meta'",
        )
        .execute(&mut conn)
        .await
        .map(|r| r > 0)
        .unwrap_or(false)
    }
}
