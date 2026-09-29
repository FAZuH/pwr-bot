use diesel::prelude::*;
use diesel_async::RunQueryDsl;

use crate::repo::error::DatabaseError;
use crate::repo::schema::feed_settings;
use crate::service::feed_settings::FeedSettingsEntity;
use crate::storage::DbPool;

#[derive(Clone)]
pub struct PgFeedSettingsRepo {
    pool: DbPool,
}

impl PgFeedSettingsRepo {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }

    pub async fn select(&self, guild_id: u64) -> Result<Option<FeedSettingsEntity>, DatabaseError> {
        let mut connection = self.pool.get().await?;
        Ok(feed_settings::table
            .find(guild_id as i64)
            .select(FeedSettingsEntity::as_select())
            .first(&mut connection)
            .await
            .optional()?)
    }

    pub async fn replace(&self, settings: &FeedSettingsEntity) -> Result<i64, DatabaseError> {
        let mut connection = self.pool.get().await?;
        let guild_id = diesel::insert_into(feed_settings::table)
            .values(settings)
            .on_conflict(feed_settings::guild_id)
            .do_update()
            .set((
                feed_settings::enabled.eq(settings.enabled),
                feed_settings::channel_id.eq(&settings.channel_id),
                feed_settings::subscribe_role_id.eq(&settings.subscribe_role_id),
                feed_settings::unsubscribe_role_id.eq(&settings.unsubscribe_role_id),
            ))
            .returning(feed_settings::guild_id)
            .get_result(&mut connection)
            .await?;
        Ok(guild_id)
    }

    pub async fn delete_all(&self) -> Result<(), DatabaseError> {
        let mut connection = self.pool.get().await?;
        diesel::delete(feed_settings::table)
            .execute(&mut connection)
            .await?;
        Ok(())
    }
}
