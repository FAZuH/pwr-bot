//! PostgreSQL database operations and implementations.

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
// PgFeedDumpRepo
// ============================================================================

#[derive(Clone)]
pub struct PgFeedDumpRepo {
    pool: DbPool,
}

impl PgFeedDumpRepo {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl FeedDumpRepository for PgFeedDumpRepo {
    async fn select_feeds(&self) -> Result<Vec<FeedEntity>, DatabaseError> {
        let mut conn = self.pool.get().await?;
        Ok(feeds::table
            .select(FeedEntity::as_select())
            .load(&mut conn)
            .await?)
    }

    async fn select_feed_items(&self) -> Result<Vec<FeedItemEntity>, DatabaseError> {
        let mut conn = self.pool.get().await?;
        Ok(feed_items::table
            .select(FeedItemEntity::as_select())
            .load(&mut conn)
            .await?)
    }

    async fn select_subscribers(&self) -> Result<Vec<SubscriberEntity>, DatabaseError> {
        let mut conn = self.pool.get().await?;
        Ok(subscribers::table
            .select(SubscriberEntity::as_select())
            .load(&mut conn)
            .await?)
    }

    async fn select_subscriptions(&self) -> Result<Vec<FeedSubscriptionEntity>, DatabaseError> {
        let mut conn = self.pool.get().await?;
        Ok(feed_subscriptions::table
            .select(FeedSubscriptionEntity::as_select())
            .load(&mut conn)
            .await?)
    }
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
// PgVoiceSessionsRepo
// ============================================================================

#[derive(Clone)]
pub struct PgVoiceSessionsRepo {
    pool: DbPool,
}

impl PgVoiceSessionsRepo {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }
}

impl_table_base!(PgVoiceSessionsRepo, voice_sessions::table);

#[async_trait::async_trait]
impl CrudTable<VoiceSessionsEntity, i32> for PgVoiceSessionsRepo {
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
        if model.id != 0 && self.select(&model.id).await?.is_some() {
            self.update(model).await?;
            return Ok(model.id);
        }
        self.insert(model).await
    }
}

#[async_trait::async_trait]
impl VoiceSessionsRepository for PgVoiceSessionsRepo {
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
                    EXTRACT(EPOCH FROM LEAST($1, CASE WHEN is_active THEN CURRENT_TIMESTAMP ELSE leave_time END))::bigint -
                    EXTRACT(EPOCH FROM GREATEST($2, join_time))::bigint
                )::bigint as total_duration
            FROM voice_sessions
            WHERE guild_id = $3
            AND join_time <= $4
            AND (is_active OR leave_time >= $5)
            GROUP BY user_id ORDER BY total_duration DESC LIMIT $6 OFFSET $7
            "#,
        )
        .bind::<diesel::sql_types::Timestamptz, _>(until_val)
        .bind::<diesel::sql_types::Timestamptz, _>(since_val)
        .bind::<diesel::sql_types::BigInt, _>(opts.guild_id as i64)
        .bind::<diesel::sql_types::Timestamptz, _>(until_val)
        .bind::<diesel::sql_types::Timestamptz, _>(since_val)
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
        let since_val = opts.since.unwrap_or(chrono::DateTime::UNIX_EPOCH);
        let until_val = opts
            .until
            .unwrap_or_else(|| chrono::Utc::now() + chrono::Duration::days(365));

        let rows: Vec<VoiceLeaderboardRow> = diesel::sql_query(
            r#"
            SELECT
                v2.user_id,
                SUM(
                    EXTRACT(EPOCH FROM LEAST(
                        CASE WHEN v1.is_active THEN CURRENT_TIMESTAMP ELSE v1.leave_time END,
                        CASE WHEN v2.is_active THEN CURRENT_TIMESTAMP ELSE v2.leave_time END
                    ))::bigint -
                    EXTRACT(EPOCH FROM GREATEST(v1.join_time, v2.join_time))::bigint
                )::bigint as total_duration
            FROM voice_sessions v1
            JOIN voice_sessions v2
                ON v1.guild_id = v2.guild_id
                AND v1.channel_id = v2.channel_id
                AND v1.user_id != v2.user_id
                AND GREATEST(v1.join_time, v2.join_time) < LEAST(
                    CASE WHEN v1.is_active THEN CURRENT_TIMESTAMP ELSE v1.leave_time END,
                    CASE WHEN v2.is_active THEN CURRENT_TIMESTAMP ELSE v2.leave_time END
                )
            WHERE v1.user_id = $1 AND v1.guild_id = $2
                AND v1.join_time >= $3 AND v2.join_time >= $4
                AND v1.join_time <= $5 AND v2.join_time <= $6
            GROUP BY v2.user_id ORDER BY total_duration DESC LIMIT $7 OFFSET $8
            "#,
        )
        .bind::<diesel::sql_types::BigInt, _>(target_user_id as i64)
        .bind::<diesel::sql_types::BigInt, _>(opts.guild_id as i64)
        .bind::<diesel::sql_types::Timestamptz, _>(since_val)
        .bind::<diesel::sql_types::Timestamptz, _>(since_val)
        .bind::<diesel::sql_types::Timestamptz, _>(until_val)
        .bind::<diesel::sql_types::Timestamptz, _>(until_val)
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
                .filter(voice_sessions::join_time.eq(join_time)),
        )
        .set(voice_sessions::leave_time.eq(leave_time))
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
                .filter(voice_sessions::join_time.eq(join_time))
                .filter(voice_sessions::is_active.eq(true)),
        )
        .set((
            voice_sessions::leave_time.eq(leave_time),
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

    async fn find_active_sessions_by_user(
        &self,
        user_id: u64,
        guild_id: u64,
    ) -> Result<Vec<VoiceSessionsEntity>, DatabaseError> {
        let mut conn = self.pool.get().await?;
        let rows: Vec<DbVoiceSession> = voice_sessions::table
            .filter(voice_sessions::is_active.eq(true))
            .filter(voice_sessions::user_id.eq(DbU64::from(user_id)))
            .filter(voice_sessions::guild_id.eq(DbU64::from(guild_id)))
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
            .filter(voice_sessions::join_time.ge(since))
            .filter(voice_sessions::join_time.le(until))
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
                DATE(join_time) as day,
                SUM(
                    CASE
                        WHEN is_active
                        THEN EXTRACT(EPOCH FROM NOW())::bigint - EXTRACT(EPOCH FROM join_time)::bigint
                        ELSE EXTRACT(EPOCH FROM leave_time)::bigint - EXTRACT(EPOCH FROM join_time)::bigint
                    END
                )::bigint as total_seconds
            FROM voice_sessions
            WHERE user_id = $1 AND guild_id = $2 AND join_time >= $3 AND join_time <= $4
            GROUP BY DATE(join_time)
            ORDER BY day
            "#,
        )
        .bind::<diesel::sql_types::BigInt, _>(user_id as i64)
        .bind::<diesel::sql_types::BigInt, _>(guild_id as i64)
        .bind::<diesel::sql_types::Timestamptz, _>(since)
        .bind::<diesel::sql_types::Timestamptz, _>(until)
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
                SUM(user_daily_total)::bigint as value
            FROM (
                SELECT
                    user_id,
                    DATE(join_time) as day,
                    SUM(
                        CASE
                            WHEN is_active
                            THEN EXTRACT(EPOCH FROM NOW())::bigint - EXTRACT(EPOCH FROM join_time)::bigint
                            ELSE EXTRACT(EPOCH FROM leave_time)::bigint - EXTRACT(EPOCH FROM join_time)::bigint
                        END
                    )::bigint as user_daily_total
                FROM voice_sessions
                WHERE guild_id = $1 AND join_time >= $2 AND join_time <= $3
                GROUP BY user_id, DATE(join_time)
            ) user_totals
            GROUP BY day
            ORDER BY day
            "#,
        )
        .bind::<diesel::sql_types::BigInt, _>(guild_id as i64)
        .bind::<diesel::sql_types::Timestamptz, _>(since)
        .bind::<diesel::sql_types::Timestamptz, _>(until)
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
                CAST(AVG(user_daily_total) AS BIGINT) as value
            FROM (
                SELECT
                    user_id,
                    DATE(join_time) as day,
                    SUM(
                        CASE
                            WHEN is_active
                            THEN EXTRACT(EPOCH FROM NOW())::bigint - EXTRACT(EPOCH FROM join_time)::bigint
                            ELSE EXTRACT(EPOCH FROM leave_time)::bigint - EXTRACT(EPOCH FROM join_time)::bigint
                        END
                    )::bigint as user_daily_total
                FROM voice_sessions
                WHERE guild_id = $1 AND join_time >= $2 AND join_time <= $3
                GROUP BY user_id, DATE(join_time)
            ) user_totals
            GROUP BY day
            ORDER BY day
            "#,
        )
        .bind::<diesel::sql_types::BigInt, _>(guild_id as i64)
        .bind::<diesel::sql_types::Timestamptz, _>(since)
        .bind::<diesel::sql_types::Timestamptz, _>(until)
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
                DATE(join_time) as day,
                COUNT(DISTINCT user_id) as value
            FROM voice_sessions
            WHERE guild_id = $1 AND join_time >= $2 AND join_time <= $3
            GROUP BY DATE(join_time)
            ORDER BY day
            "#,
        )
        .bind::<diesel::sql_types::BigInt, _>(guild_id as i64)
        .bind::<diesel::sql_types::Timestamptz, _>(since)
        .bind::<diesel::sql_types::Timestamptz, _>(until)
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
            "SELECT 1 FROM information_schema.tables WHERE table_schema = 'public' AND table_name = 'bot_meta'"
        )
        .execute(&mut conn)
        .await
        .map(|r| r > 0)
        .unwrap_or(false)
    }
}

// ============================================================================
// PgPluginKvRepo
// ============================================================================

/// Postgres-backed `PluginKvRepository` implementation.
#[derive(Clone)]
pub struct PgPluginKvRepo {
    pool: DbPool,
}

impl PgPluginKvRepo {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }
}

impl_table_base!(PgPluginKvRepo, plugin_kv::table);

#[async_trait::async_trait]
impl PluginKvRepository for PgPluginKvRepo {
    async fn get(&self, namespace: &str, key: &str) -> Result<Option<String>, DatabaseError> {
        let mut conn = self.pool.get().await?;
        Ok(plugin_kv::table
            .filter(plugin_kv::namespace.eq(namespace))
            .filter(plugin_kv::key.eq(key))
            .select(plugin_kv::value)
            .first(&mut conn)
            .await
            .optional()?)
    }

    async fn set(&self, namespace: &str, key: &str, value: &str) -> Result<(), DatabaseError> {
        let mut conn = self.pool.get().await?;
        diesel::insert_into(plugin_kv::table)
            .values((
                plugin_kv::namespace.eq(namespace),
                plugin_kv::key.eq(key),
                plugin_kv::value.eq(value),
            ))
            .on_conflict((plugin_kv::namespace, plugin_kv::key))
            .do_update()
            .set((
                plugin_kv::value.eq(value),
                plugin_kv::updated_at.eq(diesel::dsl::now),
            ))
            .execute(&mut conn)
            .await?;
        Ok(())
    }

    async fn delete(&self, namespace: &str, key: &str) -> Result<(), DatabaseError> {
        let mut conn = self.pool.get().await?;
        diesel::delete(
            plugin_kv::table
                .filter(plugin_kv::namespace.eq(namespace))
                .filter(plugin_kv::key.eq(key)),
        )
        .execute(&mut conn)
        .await?;
        Ok(())
    }
}

// ============================================================================
// PgGuildPluginRepo
// ============================================================================

/// Postgres-backed `GuildPluginRepository` implementation.
#[derive(Clone)]
pub struct PgGuildPluginRepo {
    pool: DbPool,
}

impl PgGuildPluginRepo {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }
}

impl_table_base!(PgGuildPluginRepo, guild_plugins::table);

#[async_trait::async_trait]
impl GuildPluginRepository for PgGuildPluginRepo {
    async fn list_for_guild(&self, guild_id: u64) -> Result<Vec<GuildPluginEntity>, DatabaseError> {
        let mut conn = self.pool.get().await?;
        Ok(guild_plugins::table
            .filter(guild_plugins::guild_id.eq(DbU64::from(guild_id)))
            .select(GuildPluginEntity::as_select())
            .load(&mut conn)
            .await?)
    }

    async fn set_enabled(
        &self,
        guild_id: u64,
        plugin_name: &str,
        enabled: bool,
    ) -> Result<(), DatabaseError> {
        let mut conn = self.pool.get().await?;
        diesel::insert_into(guild_plugins::table)
            .values((
                guild_plugins::guild_id.eq(DbU64::from(guild_id)),
                guild_plugins::plugin_name.eq(plugin_name),
                guild_plugins::enabled.eq(enabled),
            ))
            .on_conflict((guild_plugins::guild_id, guild_plugins::plugin_name))
            .do_update()
            .set(guild_plugins::enabled.eq(enabled))
            .execute(&mut conn)
            .await?;
        Ok(())
    }

    async fn delete(&self, guild_id: u64, plugin_name: &str) -> Result<(), DatabaseError> {
        let mut conn = self.pool.get().await?;
        diesel::delete(
            guild_plugins::table
                .filter(guild_plugins::guild_id.eq(DbU64::from(guild_id)))
                .filter(guild_plugins::plugin_name.eq(plugin_name)),
        )
        .execute(&mut conn)
        .await?;
        Ok(())
    }
}
