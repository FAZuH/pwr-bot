//! Database table operations and implementations.

use diesel::prelude::*;
use diesel_async::RunQueryDsl;

use crate::entity::*;
use crate::error::AppError;
use crate::repo::DbPool;
use crate::repo::error::DatabaseError;
use crate::repo::schema::*;
use crate::repo::traits::*;

macro_rules! impl_table_base {
    ($struct_name:ident, $table:path) => {
        #[async_trait::async_trait]
        impl TableBase for $struct_name {
            async fn create_table(&self) -> Result<(), DatabaseError> {
                // Tables are created by migrations; this is a no-op.
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
// FeedTable
// ============================================================================

#[derive(Clone)]
pub struct FeedTable {
    pool: DbPool,
}

impl FeedTable {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }
}

impl_table_base!(FeedTable, feeds::table);

#[async_trait::async_trait]
impl CrudTable<FeedEntity, i32> for FeedTable {
    async fn select_all(&self) -> Result<Vec<FeedEntity>, DatabaseError> {
        let mut conn = self.pool.get().await?;
        Ok(feeds::table
            .select(FeedEntity::as_select())
            .load(&mut conn)
            .await?)
    }

    async fn insert(&self, model: &FeedEntity) -> Result<i32, DatabaseError> {
        let mut conn = self.pool.get().await?;
        let id = diesel::insert_into(feeds::table)
            .values((
                feeds::name.eq(&model.name),
                feeds::description.eq(&model.description),
                feeds::platform_id.eq(&model.platform_id),
                feeds::source_id.eq(&model.source_id),
                feeds::items_id.eq(&model.items_id),
                feeds::source_url.eq(&model.source_url),
                feeds::cover_url.eq(&model.cover_url),
                feeds::tags.eq(&model.tags),
            ))
            .returning(feeds::id)
            .get_result(&mut conn)
            .await?;
        Ok(id)
    }

    async fn select(&self, id: &i32) -> Result<Option<FeedEntity>, DatabaseError> {
        let mut conn = self.pool.get().await?;
        Ok(feeds::table
            .find(id)
            .select(FeedEntity::as_select())
            .first(&mut conn)
            .await
            .optional()?)
    }

    async fn update(&self, model: &FeedEntity) -> Result<(), DatabaseError> {
        let mut conn = self.pool.get().await?;
        diesel::update(feeds::table.find(model.id))
            .set((
                feeds::name.eq(&model.name),
                feeds::description.eq(&model.description),
                feeds::platform_id.eq(&model.platform_id),
                feeds::source_id.eq(&model.source_id),
                feeds::items_id.eq(&model.items_id),
                feeds::source_url.eq(&model.source_url),
                feeds::cover_url.eq(&model.cover_url),
                feeds::tags.eq(&model.tags),
            ))
            .execute(&mut conn)
            .await?;
        Ok(())
    }

    async fn delete(&self, id: &i32) -> Result<(), DatabaseError> {
        let mut conn = self.pool.get().await?;
        diesel::delete(feeds::table.find(id))
            .execute(&mut conn)
            .await?;
        Ok(())
    }

    async fn replace(&self, model: &FeedEntity) -> Result<i32, DatabaseError> {
        let mut conn = self.pool.get().await?;
        let id = diesel::replace_into(feeds::table)
            .values(model)
            .returning(feeds::id)
            .get_result(&mut conn)
            .await?;
        Ok(id)
    }
}

#[async_trait::async_trait]
impl FeedRepository for FeedTable {
    async fn select_all_by_tag(&self, tag: &str) -> Result<Vec<FeedEntity>, DatabaseError> {
        let mut conn = self.pool.get().await?;
        let pattern = format!("%{}%", tag);
        Ok(feeds::table
            .filter(feeds::tags.like(pattern))
            .select(FeedEntity::as_select())
            .load(&mut conn)
            .await?)
    }

    async fn select_by_source_id(
        &self,
        platform_id: &str,
        source_id: &str,
    ) -> Result<Option<FeedEntity>, DatabaseError> {
        let mut conn = self.pool.get().await?;
        Ok(feeds::table
            .filter(feeds::platform_id.eq(platform_id))
            .filter(feeds::source_id.eq(source_id))
            .select(FeedEntity::as_select())
            .first(&mut conn)
            .await
            .optional()?)
    }

    async fn select_by_name_and_subscriber_id(
        &self,
        subscriber_id: &i32,
        name_search: &str,
        limit: Option<u32>,
    ) -> Result<Vec<FeedEntity>, DatabaseError> {
        let mut conn = self.pool.get().await?;
        let limit = limit.unwrap_or(25) as i64;
        let pattern = format!("%{}%", name_search.to_lowercase());

        Ok(feeds::table
            .filter(
                feeds::name.like(pattern).and(
                    feeds::id.eq_any(
                        feed_subscriptions::table
                            .filter(feed_subscriptions::subscriber_id.eq(subscriber_id))
                            .select(feed_subscriptions::feed_id),
                    ),
                ),
            )
            .order(feeds::name.asc())
            .limit(limit)
            .select(FeedEntity::as_select())
            .load(&mut conn)
            .await?)
    }
}

// ============================================================================
// FeedItemTable
// ============================================================================

#[derive(Clone)]
pub struct FeedItemTable {
    pool: DbPool,
}

impl FeedItemTable {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }
}

impl_table_base!(FeedItemTable, feed_items::table);

#[async_trait::async_trait]
impl CrudTable<FeedItemEntity, i32> for FeedItemTable {
    async fn select_all(&self) -> Result<Vec<FeedItemEntity>, DatabaseError> {
        let mut conn = self.pool.get().await?;
        Ok(feed_items::table
            .select(FeedItemEntity::as_select())
            .load(&mut conn)
            .await?)
    }

    async fn insert(&self, model: &FeedItemEntity) -> Result<i32, DatabaseError> {
        let mut conn = self.pool.get().await?;
        let id = diesel::insert_into(feed_items::table)
            .values((
                feed_items::feed_id.eq(model.feed_id),
                feed_items::description.eq(&model.description),
                feed_items::published.eq(model.published),
            ))
            .returning(feed_items::id)
            .get_result(&mut conn)
            .await?;
        Ok(id)
    }

    async fn select(&self, id: &i32) -> Result<Option<FeedItemEntity>, DatabaseError> {
        let mut conn = self.pool.get().await?;
        Ok(feed_items::table
            .find(id)
            .select(FeedItemEntity::as_select())
            .first(&mut conn)
            .await
            .optional()?)
    }

    async fn update(&self, model: &FeedItemEntity) -> Result<(), DatabaseError> {
        let mut conn = self.pool.get().await?;
        diesel::update(feed_items::table.find(model.id))
            .set((
                feed_items::feed_id.eq(model.feed_id),
                feed_items::description.eq(&model.description),
                feed_items::published.eq(model.published),
            ))
            .execute(&mut conn)
            .await?;
        Ok(())
    }

    async fn delete(&self, id: &i32) -> Result<(), DatabaseError> {
        let mut conn = self.pool.get().await?;
        diesel::delete(feed_items::table.find(id))
            .execute(&mut conn)
            .await?;
        Ok(())
    }

    async fn replace(&self, model: &FeedItemEntity) -> Result<i32, DatabaseError> {
        self.insert(model).await
    }
}

#[async_trait::async_trait]
impl FeedItemRepository for FeedItemTable {
    async fn select_latest_by_feed_id(
        &self,
        feed_id: i32,
    ) -> Result<Option<FeedItemEntity>, DatabaseError> {
        let mut conn = self.pool.get().await?;
        Ok(feed_items::table
            .filter(feed_items::feed_id.eq(feed_id))
            .order(feed_items::published.desc())
            .select(FeedItemEntity::as_select())
            .first(&mut conn)
            .await
            .optional()?)
    }

    async fn select_all_by_feed_id(
        &self,
        feed_id: i32,
    ) -> Result<Vec<FeedItemEntity>, DatabaseError> {
        let mut conn = self.pool.get().await?;
        Ok(feed_items::table
            .filter(feed_items::feed_id.eq(feed_id))
            .order(feed_items::published.desc())
            .select(FeedItemEntity::as_select())
            .load(&mut conn)
            .await?)
    }

    async fn delete_all_by_feed_id(&self, feed_id: i32) -> Result<(), DatabaseError> {
        let mut conn = self.pool.get().await?;
        diesel::delete(feed_items::table.filter(feed_items::feed_id.eq(feed_id)))
            .execute(&mut conn)
            .await?;
        Ok(())
    }
}

// ============================================================================
// SubscriberTable
// ============================================================================

#[derive(Clone)]
pub struct SubscriberTable {
    pool: DbPool,
}

impl SubscriberTable {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }
}

impl_table_base!(SubscriberTable, subscribers::table);

#[async_trait::async_trait]
impl CrudTable<SubscriberEntity, i32> for SubscriberTable {
    async fn select_all(&self) -> Result<Vec<SubscriberEntity>, DatabaseError> {
        let mut conn = self.pool.get().await?;
        Ok(subscribers::table
            .select(SubscriberEntity::as_select())
            .load(&mut conn)
            .await?)
    }

    async fn insert(&self, model: &SubscriberEntity) -> Result<i32, DatabaseError> {
        let mut conn = self.pool.get().await?;
        let id = diesel::insert_into(subscribers::table)
            .values((
                subscribers::type_.eq(model.r#type),
                subscribers::target_id.eq(&model.target_id),
            ))
            .returning(subscribers::id)
            .get_result(&mut conn)
            .await?;
        Ok(id)
    }

    async fn select(&self, id: &i32) -> Result<Option<SubscriberEntity>, DatabaseError> {
        let mut conn = self.pool.get().await?;
        Ok(subscribers::table
            .find(id)
            .select(SubscriberEntity::as_select())
            .first(&mut conn)
            .await
            .optional()?)
    }

    async fn update(&self, model: &SubscriberEntity) -> Result<(), DatabaseError> {
        let mut conn = self.pool.get().await?;
        diesel::update(subscribers::table.find(model.id))
            .set((
                subscribers::type_.eq(model.r#type),
                subscribers::target_id.eq(&model.target_id),
            ))
            .execute(&mut conn)
            .await?;
        Ok(())
    }

    async fn delete(&self, id: &i32) -> Result<(), DatabaseError> {
        let mut conn = self.pool.get().await?;
        diesel::delete(subscribers::table.find(id))
            .execute(&mut conn)
            .await?;
        Ok(())
    }

    async fn replace(&self, model: &SubscriberEntity) -> Result<i32, DatabaseError> {
        self.insert(model).await
    }
}

#[async_trait::async_trait]
impl SubscriberRepository for SubscriberTable {
    async fn select_all_by_type_and_feed(
        &self,
        r#type: SubscriberType,
        feed_id: i32,
    ) -> Result<Vec<SubscriberEntity>, DatabaseError> {
        let mut conn = self.pool.get().await?;
        Ok(subscribers::table
            .filter(subscribers::type_.eq(r#type))
            .filter(
                subscribers::id.eq_any(
                    feed_subscriptions::table
                        .filter(feed_subscriptions::feed_id.eq(feed_id))
                        .select(feed_subscriptions::subscriber_id),
                ),
            )
            .select(SubscriberEntity::as_select())
            .load(&mut conn)
            .await?)
    }

    async fn select_by_type_and_target(
        &self,
        r#type: &SubscriberType,
        target_id: &str,
    ) -> Result<Option<SubscriberEntity>, DatabaseError> {
        let mut conn = self.pool.get().await?;
        Ok(subscribers::table
            .filter(subscribers::type_.eq(r#type))
            .filter(subscribers::target_id.eq(target_id))
            .select(SubscriberEntity::as_select())
            .first(&mut conn)
            .await
            .optional()?)
    }
}

// ============================================================================
// FeedSubscriptionTable
// ============================================================================

#[derive(Clone)]
pub struct FeedSubscriptionTable {
    pool: DbPool,
}

impl FeedSubscriptionTable {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }
}

impl_table_base!(FeedSubscriptionTable, feed_subscriptions::table);

#[async_trait::async_trait]
impl CrudTable<FeedSubscriptionEntity, i32> for FeedSubscriptionTable {
    async fn select_all(&self) -> Result<Vec<FeedSubscriptionEntity>, DatabaseError> {
        let mut conn = self.pool.get().await?;
        Ok(feed_subscriptions::table
            .select(FeedSubscriptionEntity::as_select())
            .load(&mut conn)
            .await?)
    }

    async fn insert(&self, model: &FeedSubscriptionEntity) -> Result<i32, DatabaseError> {
        let mut conn = self.pool.get().await?;
        let id = diesel::insert_into(feed_subscriptions::table)
            .values((
                feed_subscriptions::feed_id.eq(model.feed_id),
                feed_subscriptions::subscriber_id.eq(model.subscriber_id),
            ))
            .returning(feed_subscriptions::id)
            .get_result(&mut conn)
            .await?;
        Ok(id)
    }

    async fn select(&self, id: &i32) -> Result<Option<FeedSubscriptionEntity>, DatabaseError> {
        let mut conn = self.pool.get().await?;
        Ok(feed_subscriptions::table
            .find(id)
            .select(FeedSubscriptionEntity::as_select())
            .first(&mut conn)
            .await
            .optional()?)
    }

    async fn update(&self, model: &FeedSubscriptionEntity) -> Result<(), DatabaseError> {
        let mut conn = self.pool.get().await?;
        diesel::update(feed_subscriptions::table.find(model.id))
            .set((
                feed_subscriptions::feed_id.eq(model.feed_id),
                feed_subscriptions::subscriber_id.eq(model.subscriber_id),
            ))
            .execute(&mut conn)
            .await?;
        Ok(())
    }

    async fn delete(&self, id: &i32) -> Result<(), DatabaseError> {
        let mut conn = self.pool.get().await?;
        diesel::delete(feed_subscriptions::table.find(id))
            .execute(&mut conn)
            .await?;
        Ok(())
    }

    async fn replace(&self, model: &FeedSubscriptionEntity) -> Result<i32, DatabaseError> {
        self.insert(model).await
    }
}

#[async_trait::async_trait]
impl FeedSubscriptionRepository for FeedSubscriptionTable {
    async fn select_all_by_feed_id(
        &self,
        feed_id: i32,
    ) -> Result<Vec<FeedSubscriptionEntity>, DatabaseError> {
        let mut conn = self.pool.get().await?;
        Ok(feed_subscriptions::table
            .filter(feed_subscriptions::feed_id.eq(feed_id))
            .select(FeedSubscriptionEntity::as_select())
            .load(&mut conn)
            .await?)
    }

    async fn select_all_by_subscriber_id(
        &self,
        subscriber_id: i32,
    ) -> Result<Vec<FeedSubscriptionEntity>, DatabaseError> {
        let mut conn = self.pool.get().await?;
        Ok(feed_subscriptions::table
            .filter(feed_subscriptions::subscriber_id.eq(subscriber_id))
            .select(FeedSubscriptionEntity::as_select())
            .load(&mut conn)
            .await?)
    }

    async fn count_by_subscriber_id(&self, subscriber_id: i32) -> Result<u32, DatabaseError> {
        let mut conn = self.pool.get().await?;
        let count: i64 = feed_subscriptions::table
            .filter(feed_subscriptions::subscriber_id.eq(subscriber_id))
            .count()
            .get_result(&mut conn)
            .await?;
        Ok(count as u32)
    }

    async fn select_paginated_by_subscriber_id(
        &self,
        subscriber_id: i32,
        page: u32,
        per_page: u32,
    ) -> Result<Vec<FeedSubscriptionEntity>, DatabaseError> {
        let mut conn = self.pool.get().await?;
        let limit = per_page as i64;
        let offset = (per_page * page) as i64;
        Ok(feed_subscriptions::table
            .filter(feed_subscriptions::subscriber_id.eq(subscriber_id))
            .order(feed_subscriptions::id.asc())
            .limit(limit)
            .offset(offset)
            .select(FeedSubscriptionEntity::as_select())
            .load(&mut conn)
            .await?)
    }

    async fn select_paginated_with_latest_by_subscriber_id(
        &self,
        subscriber_id: i32,
        page: u32,
        per_page: u32,
    ) -> Result<Vec<FeedWithLatestItemRow>, DatabaseError> {
        let mut conn = self.pool.get().await?;
        let limit = per_page as i64;
        let offset = (per_page * page) as i64;

        let rows = diesel::sql_query(
            r#"
            SELECT
                f.id, f.name, f.description, f.platform_id, f.source_id, f.items_id, f.source_url, f.cover_url, f.tags,
                fi.id as item_id, fi.description as item_description, fi.published as item_published
            FROM feed_subscriptions fs
            JOIN feeds f ON fs.feed_id = f.id
            LEFT JOIN feed_items fi ON fi.id = (
                SELECT id FROM feed_items WHERE feed_id = f.id ORDER BY published DESC LIMIT 1
            )
            WHERE fs.subscriber_id = ?
            ORDER BY f.name
            LIMIT ? OFFSET ?
            "#,
        )
        .bind::<diesel::sql_types::Integer, _>(subscriber_id)
        .bind::<diesel::sql_types::BigInt, _>(limit)
        .bind::<diesel::sql_types::BigInt, _>(offset)
        .load::<FeedWithLatestItemRow>(&mut conn)
        .await?;
        Ok(rows)
    }

    async fn exists_by_feed_id(&self, feed_id: i32) -> Result<bool, DatabaseError> {
        let mut conn = self.pool.get().await?;
        let count: i64 = feed_subscriptions::table
            .filter(feed_subscriptions::feed_id.eq(feed_id))
            .count()
            .get_result(&mut conn)
            .await?;
        Ok(count > 0)
    }

    async fn delete_subscription(
        &self,
        feed_id: i32,
        subscriber_id: i32,
    ) -> Result<bool, DatabaseError> {
        let mut conn = self.pool.get().await?;
        let affected = diesel::delete(
            feed_subscriptions::table
                .filter(feed_subscriptions::feed_id.eq(feed_id))
                .filter(feed_subscriptions::subscriber_id.eq(subscriber_id)),
        )
        .execute(&mut conn)
        .await?;
        Ok(affected > 0)
    }

    async fn delete_all_by_feed_id(&self, feed_id: i32) -> Result<(), DatabaseError> {
        let mut conn = self.pool.get().await?;
        diesel::delete(feed_subscriptions::table.filter(feed_subscriptions::feed_id.eq(feed_id)))
            .execute(&mut conn)
            .await?;
        Ok(())
    }

    async fn delete_all_by_subscriber_id(&self, subscriber_id: i32) -> Result<(), DatabaseError> {
        let mut conn = self.pool.get().await?;
        diesel::delete(
            feed_subscriptions::table.filter(feed_subscriptions::subscriber_id.eq(subscriber_id)),
        )
        .execute(&mut conn)
        .await?;
        Ok(())
    }
}

// ============================================================================
// ServerSettingsTable
// ============================================================================

#[derive(Clone)]
pub struct ServerSettingsTable {
    pool: DbPool,
}

impl ServerSettingsTable {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }
}

impl_table_base!(ServerSettingsTable, server_settings::table);

#[async_trait::async_trait]
impl CrudTable<ServerSettingsEntity, u64> for ServerSettingsTable {
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
        let mut conn = self.pool.get().await?;
        let guild_id: DbU64 = diesel::replace_into(server_settings::table)
            .values(model)
            .returning(server_settings::guild_id)
            .get_result(&mut conn)
            .await?;
        Ok(guild_id.into())
    }
}

#[async_trait::async_trait]
impl ServerSettingsRepository for ServerSettingsTable {}

// ============================================================================
// VoiceSessionsTable
// ============================================================================

#[derive(Clone)]
pub struct VoiceSessionsTable {
    pool: DbPool,
}

impl VoiceSessionsTable {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }
}

impl_table_base!(VoiceSessionsTable, voice_sessions::table);

#[async_trait::async_trait]
impl CrudTable<VoiceSessionsEntity, i32> for VoiceSessionsTable {
    async fn select_all(&self) -> Result<Vec<VoiceSessionsEntity>, DatabaseError> {
        let mut conn = self.pool.get().await?;
        let rows: Vec<DbVoiceSession> = voice_sessions::table
            .select(DbVoiceSession::as_select())
            .load(&mut conn)
            .await?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn insert(&self, model: &VoiceSessionsEntity) -> Result<i32, DatabaseError> {
        let mut conn = self.pool.get().await?;
        let id = diesel::insert_into(voice_sessions::table)
            .values(&model.to_insertable())
            .returning(voice_sessions::id)
            .get_result(&mut conn)
            .await?;
        Ok(id)
    }

    async fn select(&self, id: &i32) -> Result<Option<VoiceSessionsEntity>, DatabaseError> {
        let mut conn = self.pool.get().await?;
        let result: Option<DbVoiceSession> = voice_sessions::table
            .find(id)
            .select(DbVoiceSession::as_select())
            .first(&mut conn)
            .await
            .optional()?;
        Ok(result.map(Into::into))
    }

    async fn update(&self, model: &VoiceSessionsEntity) -> Result<(), DatabaseError> {
        let mut conn = self.pool.get().await?;
        diesel::update(voice_sessions::table.find(model.id))
            .set(&model.to_insertable())
            .execute(&mut conn)
            .await?;
        Ok(())
    }

    async fn delete(&self, id: &i32) -> Result<(), DatabaseError> {
        let mut conn = self.pool.get().await?;
        diesel::delete(voice_sessions::table.find(id))
            .execute(&mut conn)
            .await?;
        Ok(())
    }

    async fn replace(&self, model: &VoiceSessionsEntity) -> Result<i32, DatabaseError> {
        let mut conn = self.pool.get().await?;
        let id = diesel::replace_into(voice_sessions::table)
            .values(&model.to_insertable())
            .returning(voice_sessions::id)
            .get_result(&mut conn)
            .await?;
        Ok(id)
    }
}

#[async_trait::async_trait]
impl VoiceSessionsRepository for VoiceSessionsTable {
    async fn get_leaderboard_opt(
        &self,
        opts: &VoiceLeaderboardOpt,
    ) -> Result<Vec<VoiceLeaderboardEntry>, DatabaseError> {
        let mut conn = self.pool.get().await?;
        let limit = opts.limit.unwrap_or(10) as i64;
        let offset = opts.offset.unwrap_or(0) as i64;
        let since_val = opts.since.unwrap_or(chrono::DateTime::UNIX_EPOCH);
        let until_val = opts
            .until
            .unwrap_or_else(|| chrono::Utc::now() + chrono::Duration::days(365));

        let rows: Vec<VoiceLeaderboardRow> = diesel::sql_query(
            r#"
            SELECT
                user_id,
                SUM(
                    strftime('%s', MIN(?, CASE WHEN is_active = 1 THEN CURRENT_TIMESTAMP ELSE leave_time END)) -
                    strftime('%s', MAX(?, join_time))
                ) as total_duration
            FROM voice_sessions
            WHERE guild_id = ?
            AND join_time <= ?
            AND (is_active = 1 OR leave_time >= ?)
            GROUP BY user_id ORDER BY total_duration DESC LIMIT ? OFFSET ?
            "#,
        )
        .bind::<diesel::sql_types::Timestamp, _>(until_val.naive_utc())
        .bind::<diesel::sql_types::Timestamp, _>(since_val.naive_utc())
        .bind::<diesel::sql_types::BigInt, _>(opts.guild_id as i64)
        .bind::<diesel::sql_types::Timestamp, _>(until_val.naive_utc())
        .bind::<diesel::sql_types::Timestamp, _>(since_val.naive_utc())
        .bind::<diesel::sql_types::BigInt, _>(limit)
        .bind::<diesel::sql_types::BigInt, _>(offset)
        .load(&mut conn)
        .await?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn get_leaderboard(
        &self,
        guild_id: u64,
        limit: u32,
    ) -> Result<Vec<VoiceLeaderboardEntry>, DatabaseError> {
        let opts = VoiceLeaderboardOptBuilder::default()
            .guild_id(guild_id)
            .limit(Some(limit))
            .build()
            .map_err(AppError::from)?;
        self.get_leaderboard_opt(&opts).await
    }

    async fn get_leaderboard_with_offset(
        &self,
        guild_id: u64,
        offset: u32,
        limit: u32,
    ) -> Result<Vec<VoiceLeaderboardEntry>, DatabaseError> {
        let opts = VoiceLeaderboardOptBuilder::default()
            .guild_id(guild_id)
            .offset(Some(offset))
            .limit(Some(limit))
            .build()
            .map_err(AppError::from)?;
        self.get_leaderboard_opt(&opts).await
    }

    async fn get_partner_leaderboard(
        &self,
        opts: &VoiceLeaderboardOpt,
        target_user_id: u64,
    ) -> Result<Vec<VoiceLeaderboardEntry>, DatabaseError> {
        let mut conn = self.pool.get().await?;
        let limit = opts.limit.unwrap_or(10) as i64;
        let offset = opts.offset.unwrap_or(0) as i64;
        let since_val = opts
            .since
            .map(|s| s.naive_utc())
            .unwrap_or_else(|| chrono::DateTime::UNIX_EPOCH.naive_utc());
        let until_val = opts
            .until
            .map(|u| u.naive_utc())
            .unwrap_or_else(|| (chrono::Utc::now() + chrono::Duration::days(365)).naive_utc());

        let rows: Vec<VoiceLeaderboardRow> = diesel::sql_query(
            r#"
            SELECT
                v2.user_id,
                SUM(
                    strftime('%s', MIN(
                        CASE WHEN v1.is_active = 1 THEN CURRENT_TIMESTAMP ELSE v1.leave_time END,
                        CASE WHEN v2.is_active = 1 THEN CURRENT_TIMESTAMP ELSE v2.leave_time END
                    )) -
                    strftime('%s', MAX(v1.join_time, v2.join_time))
                ) as total_duration
            FROM voice_sessions v1
            JOIN voice_sessions v2
                ON v1.guild_id = v2.guild_id
                AND v1.channel_id = v2.channel_id
                AND v1.user_id != v2.user_id
                AND MAX(v1.join_time, v2.join_time) < MIN(
                    CASE WHEN v1.is_active = 1 THEN CURRENT_TIMESTAMP ELSE v1.leave_time END,
                    CASE WHEN v2.is_active = 1 THEN CURRENT_TIMESTAMP ELSE v2.leave_time END
                )
            WHERE v1.user_id = ? AND v1.guild_id = ?
                AND v1.join_time >= ? AND v2.join_time >= ?
                AND v1.join_time <= ? AND v2.join_time <= ?
            GROUP BY v2.user_id ORDER BY total_duration DESC LIMIT ? OFFSET ?
            "#,
        )
        .bind::<diesel::sql_types::BigInt, _>(target_user_id as i64)
        .bind::<diesel::sql_types::BigInt, _>(opts.guild_id as i64)
        .bind::<diesel::sql_types::Timestamp, _>(since_val)
        .bind::<diesel::sql_types::Timestamp, _>(since_val)
        .bind::<diesel::sql_types::Timestamp, _>(until_val)
        .bind::<diesel::sql_types::Timestamp, _>(until_val)
        .bind::<diesel::sql_types::BigInt, _>(limit)
        .bind::<diesel::sql_types::BigInt, _>(offset)
        .load(&mut conn)
        .await?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn update_leave_time(
        &self,
        user_id: u64,
        channel_id: u64,
        join_time: &chrono::DateTime<chrono::Utc>,
        leave_time: &chrono::DateTime<chrono::Utc>,
    ) -> Result<(), DatabaseError> {
        let mut conn = self.pool.get().await?;
        diesel::update(
            voice_sessions::table
                .filter(voice_sessions::user_id.eq(DbU64::from(user_id)))
                .filter(voice_sessions::channel_id.eq(DbU64::from(channel_id)))
                .filter(voice_sessions::join_time.eq(join_time.naive_utc())),
        )
        .set(voice_sessions::leave_time.eq(leave_time.naive_utc()))
        .execute(&mut conn)
        .await?;
        Ok(())
    }

    async fn close_session(
        &self,
        user_id: u64,
        channel_id: u64,
        join_time: &chrono::DateTime<chrono::Utc>,
        leave_time: &chrono::DateTime<chrono::Utc>,
    ) -> Result<(), DatabaseError> {
        let mut conn = self.pool.get().await?;
        diesel::update(
            voice_sessions::table
                .filter(voice_sessions::user_id.eq(DbU64::from(user_id)))
                .filter(voice_sessions::channel_id.eq(DbU64::from(channel_id)))
                .filter(voice_sessions::join_time.eq(join_time.naive_utc())),
        )
        .set((
            voice_sessions::leave_time.eq(leave_time.naive_utc()),
            voice_sessions::is_active.eq(false),
        ))
        .execute(&mut conn)
        .await?;
        Ok(())
    }

    async fn find_active_sessions(&self) -> Result<Vec<VoiceSessionsEntity>, DatabaseError> {
        let mut conn = self.pool.get().await?;
        let rows: Vec<DbVoiceSession> = voice_sessions::table
            .filter(voice_sessions::is_active.eq(true))
            .select(DbVoiceSession::as_select())
            .load(&mut conn)
            .await?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn get_sessions_in_range(
        &self,
        guild_id: u64,
        user_id: Option<u64>,
        since: &chrono::DateTime<chrono::Utc>,
        until: &chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<VoiceSessionsEntity>, DatabaseError> {
        let mut conn = self.pool.get().await?;
        let mut query = voice_sessions::table
            .filter(voice_sessions::guild_id.eq(DbU64::from(guild_id)))
            .filter(voice_sessions::join_time.ge(since.naive_utc()))
            .filter(voice_sessions::join_time.le(until.naive_utc()))
            .into_boxed();

        if let Some(uid) = user_id {
            query = query.filter(voice_sessions::user_id.eq(DbU64::from(uid)));
        }

        let rows: Vec<DbVoiceSession> = query
            .order(voice_sessions::join_time.asc())
            .select(DbVoiceSession::as_select())
            .load(&mut conn)
            .await?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn get_user_daily_activity(
        &self,
        user_id: u64,
        guild_id: u64,
        since: &chrono::DateTime<chrono::Utc>,
        until: &chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<VoiceDailyActivity>, DatabaseError> {
        let mut conn = self.pool.get().await?;
        let rows = diesel::sql_query(
            r#"
            SELECT
                date(join_time) as day,
                SUM(
                    CASE
                        WHEN is_active = 1
                        THEN strftime('%s', 'now') - strftime('%s', join_time)
                        ELSE strftime('%s', leave_time) - strftime('%s', join_time)
                    END
                ) as total_seconds
            FROM voice_sessions
            WHERE user_id = ? AND guild_id = ? AND join_time >= ? AND join_time <= ?
            GROUP BY date(join_time)
            ORDER BY day
            "#,
        )
        .bind::<diesel::sql_types::BigInt, _>(user_id as i64)
        .bind::<diesel::sql_types::BigInt, _>(guild_id as i64)
        .bind::<diesel::sql_types::Timestamp, _>(since.naive_utc())
        .bind::<diesel::sql_types::Timestamp, _>(until.naive_utc())
        .load::<VoiceDailyActivity>(&mut conn)
        .await?;
        Ok(rows)
    }

    async fn get_guild_daily_total_time(
        &self,
        guild_id: u64,
        since: &chrono::DateTime<chrono::Utc>,
        until: &chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<GuildDailyStats>, DatabaseError> {
        let mut conn = self.pool.get().await?;
        let rows = diesel::sql_query(
            r#"
            SELECT
                day,
                SUM(user_daily_total) as value
            FROM (
                SELECT
                    user_id,
                    date(join_time) as day,
                    SUM(
                        CASE
                            WHEN is_active = 1
                            THEN strftime('%s', 'now') - strftime('%s', join_time)
                            ELSE strftime('%s', leave_time) - strftime('%s', join_time)
                        END
                    ) as user_daily_total
                FROM voice_sessions
                WHERE guild_id = ? AND join_time >= ? AND join_time <= ?
                GROUP BY user_id, date(join_time)
            ) user_totals
            GROUP BY day
            ORDER BY day
            "#,
        )
        .bind::<diesel::sql_types::BigInt, _>(guild_id as i64)
        .bind::<diesel::sql_types::Timestamp, _>(since.naive_utc())
        .bind::<diesel::sql_types::Timestamp, _>(until.naive_utc())
        .load::<GuildDailyStats>(&mut conn)
        .await?;
        Ok(rows)
    }

    async fn get_guild_daily_average_time(
        &self,
        guild_id: u64,
        since: &chrono::DateTime<chrono::Utc>,
        until: &chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<GuildDailyStats>, DatabaseError> {
        let mut conn = self.pool.get().await?;
        let rows = diesel::sql_query(
            r#"
            SELECT
                day,
                CAST(AVG(user_daily_total) AS INTEGER) as value
            FROM (
                SELECT
                    user_id,
                    date(join_time) as day,
                    SUM(
                        CASE
                            WHEN is_active = 1
                            THEN strftime('%s', 'now') - strftime('%s', join_time)
                            ELSE strftime('%s', leave_time) - strftime('%s', join_time)
                        END
                    ) as user_daily_total
                FROM voice_sessions
                WHERE guild_id = ? AND join_time >= ? AND join_time <= ?
                GROUP BY user_id, date(join_time)
            ) user_totals
            GROUP BY day
            ORDER BY day
            "#,
        )
        .bind::<diesel::sql_types::BigInt, _>(guild_id as i64)
        .bind::<diesel::sql_types::Timestamp, _>(since.naive_utc())
        .bind::<diesel::sql_types::Timestamp, _>(until.naive_utc())
        .load::<GuildDailyStats>(&mut conn)
        .await?;
        Ok(rows)
    }

    async fn get_guild_daily_user_count(
        &self,
        guild_id: u64,
        since: &chrono::DateTime<chrono::Utc>,
        until: &chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<GuildDailyStats>, DatabaseError> {
        let mut conn = self.pool.get().await?;
        let rows = diesel::sql_query(
            r#"
            SELECT
                date(join_time) as day,
                COUNT(DISTINCT user_id) as value
            FROM voice_sessions
            WHERE guild_id = ? AND join_time >= ? AND join_time <= ?
            GROUP BY date(join_time)
            ORDER BY day
            "#,
        )
        .bind::<diesel::sql_types::BigInt, _>(guild_id as i64)
        .bind::<diesel::sql_types::Timestamp, _>(since.naive_utc())
        .bind::<diesel::sql_types::Timestamp, _>(until.naive_utc())
        .load::<GuildDailyStats>(&mut conn)
        .await?;
        Ok(rows)
    }
}

impl From<VoiceLeaderboardOptBuilderError> for AppError {
    fn from(value: VoiceLeaderboardOptBuilderError) -> Self {
        AppError::internal_with_ref(value)
    }
}

// ============================================================================
// BotMetaTable
// ============================================================================

#[derive(Clone)]
pub struct BotMetaTable {
    pool: DbPool,
}

impl BotMetaTable {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }
}

impl_table_base!(BotMetaTable, bot_meta::table);

#[async_trait::async_trait]
impl CrudTable<BotMetaEntity, String> for BotMetaTable {
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
        let mut conn = self.pool.get().await?;
        let key = diesel::replace_into(bot_meta::table)
            .values(model)
            .returning(bot_meta::key)
            .get_result(&mut conn)
            .await?;
        Ok(key)
    }
}

#[async_trait::async_trait]
impl BotMetaRepository for BotMetaTable {
    async fn table_exists(&self) -> bool {
        let mut conn = match self.pool.get().await {
            Ok(c) => c,
            Err(_) => return false,
        };
        diesel::sql_query("SELECT name FROM sqlite_master WHERE type='table' AND name='bot_meta'")
            .execute(&mut conn)
            .await
            .map(|r| r > 0)
            .unwrap_or(false)
    }
}
