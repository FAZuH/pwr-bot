#![allow(clippy::redundant_field_names)]

use diesel::prelude::*;
use diesel::sql_types::BigInt;
use diesel_async::RunQueryDsl;

use crate::GuildDailyStats;
use crate::GuildStatType;
use crate::VoiceDailyActivity;
use crate::VoiceLeaderboardEntry;
use crate::VoiceLeaderboardOpt;
use crate::VoiceSessionsEntity;
use crate::VoiceSettingsEntity;
use crate::entity::DbVoiceSession;
use crate::entity::VoiceLeaderboardRow;
use crate::repo::error::DatabaseError;
use crate::repo::schema::voice_sessions;
use crate::repo::schema::voice_settings;
use crate::repo::traits::VoiceSessionsRepository;
use crate::repo::traits::VoiceSettingsRepository;
use crate::storage::DbPool;

#[derive(Clone)]
pub struct PgVoiceSessionsRepo {
    pool: DbPool,
}

impl PgVoiceSessionsRepo {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl VoiceSessionsRepository for PgVoiceSessionsRepo {
    async fn select_all(&self) -> Result<Vec<VoiceSessionsEntity>, DatabaseError> {
        let mut connection = self.pool.get().await?;
        Ok(voice_sessions::table
            .select(DbVoiceSession::as_select())
            .load(&mut connection)
            .await?
            .into_iter()
            .map(VoiceSessionsEntity::from)
            .collect())
    }

    async fn insert(&self, model: &VoiceSessionsEntity) -> Result<i32, DatabaseError> {
        let mut connection = self.pool.get().await?;
        let id = diesel::insert_into(voice_sessions::table)
            .values(&model.to_insertable())
            .returning(voice_sessions::id)
            .get_result(&mut connection)
            .await?;
        Ok(id)
    }

    async fn replace(&self, model: &VoiceSessionsEntity) -> Result<i32, DatabaseError> {
        if model.id != 0 {
            let mut connection = self.pool.get().await?;
            let exists = voice_sessions::table
                .find(model.id)
                .select(voice_sessions::id)
                .first::<i32>(&mut connection)
                .await
                .optional()?
                .is_some();
            if exists {
                diesel::update(voice_sessions::table.find(model.id))
                    .set(&model.to_insertable())
                    .execute(&mut connection)
                    .await?;
                return Ok(model.id);
            }
        }
        self.insert(model).await
    }

    async fn delete_all(&self) -> Result<(), DatabaseError> {
        let mut connection = self.pool.get().await?;
        diesel::delete(voice_sessions::table)
            .execute(&mut connection)
            .await?;
        Ok(())
    }

    async fn get_leaderboard_opt(
        &self,
        options: &VoiceLeaderboardOpt,
    ) -> Result<Vec<VoiceLeaderboardEntry>, DatabaseError> {
        let mut connection = self.pool.get().await?;
        let limit = options.limit.unwrap_or(10) as i64;
        let offset = options.offset.unwrap_or(0) as i64;
        let since = options.since.unwrap_or(chrono::DateTime::UNIX_EPOCH);
        let until = options
            .until
            .unwrap_or_else(|| chrono::Utc::now() + chrono::Duration::days(365));
        let rows: Vec<VoiceLeaderboardRow> = diesel::sql_query(
            r#"
            SELECT user_id,
                SUM(
                    EXTRACT(
                        EPOCH FROM LEAST(
                            $1,
                            CASE WHEN is_active THEN CURRENT_TIMESTAMP ELSE leave_time END
                        )
                    )::bigint -
                    EXTRACT(EPOCH FROM GREATEST($2, join_time))::bigint
                )::bigint AS total_duration
            FROM voice_sessions
            WHERE guild_id = $3
              AND join_time <= $4
              AND (is_active OR leave_time >= $5)
            GROUP BY user_id
            ORDER BY total_duration DESC
            LIMIT $6 OFFSET $7
            "#,
        )
        .bind::<diesel::sql_types::Timestamptz, _>(until)
        .bind::<diesel::sql_types::Timestamptz, _>(since)
        .bind::<diesel::sql_types::BigInt, _>(options.guild_id as i64)
        .bind::<diesel::sql_types::Timestamptz, _>(until)
        .bind::<diesel::sql_types::Timestamptz, _>(since)
        .bind::<diesel::sql_types::BigInt, _>(limit)
        .bind::<diesel::sql_types::BigInt, _>(offset)
        .load(&mut connection)
        .await?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn get_partner_leaderboard(
        &self,
        options: &VoiceLeaderboardOpt,
        target_user_id: u64,
    ) -> Result<Vec<VoiceLeaderboardEntry>, DatabaseError> {
        let mut connection = self.pool.get().await?;
        let limit = options.limit.unwrap_or(10) as i64;
        let offset = options.offset.unwrap_or(0) as i64;
        let since = options.since.unwrap_or(chrono::DateTime::UNIX_EPOCH);
        let until = options
            .until
            .unwrap_or_else(|| chrono::Utc::now() + chrono::Duration::days(365));
        let rows: Vec<VoiceLeaderboardRow> = diesel::sql_query(
            r#"
            SELECT v2.user_id,
                SUM(
                    EXTRACT(EPOCH FROM LEAST(
                        CASE WHEN v1.is_active THEN CURRENT_TIMESTAMP ELSE v1.leave_time END,
                        CASE WHEN v2.is_active THEN CURRENT_TIMESTAMP ELSE v2.leave_time END
                    ))::bigint -
                    EXTRACT(EPOCH FROM GREATEST(v1.join_time, v2.join_time))::bigint
                )::bigint AS total_duration
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
            GROUP BY v2.user_id
            ORDER BY total_duration DESC
            LIMIT $7 OFFSET $8
            "#,
        )
        .bind::<diesel::sql_types::BigInt, _>(target_user_id as i64)
        .bind::<diesel::sql_types::BigInt, _>(options.guild_id as i64)
        .bind::<diesel::sql_types::Timestamptz, _>(since)
        .bind::<diesel::sql_types::Timestamptz, _>(since)
        .bind::<diesel::sql_types::Timestamptz, _>(until)
        .bind::<diesel::sql_types::Timestamptz, _>(until)
        .bind::<diesel::sql_types::BigInt, _>(limit)
        .bind::<diesel::sql_types::BigInt, _>(offset)
        .load(&mut connection)
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
        let mut connection = self.pool.get().await?;
        diesel::update(
            voice_sessions::table
                .filter(voice_sessions::user_id.eq(user_id as i64))
                .filter(voice_sessions::channel_id.eq(channel_id as i64))
                .filter(voice_sessions::join_time.eq(join_time)),
        )
        .set(voice_sessions::leave_time.eq(leave_time))
        .execute(&mut connection)
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
        let mut connection = self.pool.get().await?;
        diesel::update(
            voice_sessions::table
                .filter(voice_sessions::user_id.eq(user_id as i64))
                .filter(voice_sessions::channel_id.eq(channel_id as i64))
                .filter(voice_sessions::join_time.eq(join_time))
                .filter(voice_sessions::is_active.eq(true)),
        )
        .set((
            voice_sessions::leave_time.eq(leave_time),
            voice_sessions::is_active.eq(false),
        ))
        .execute(&mut connection)
        .await?;
        Ok(())
    }

    async fn find_active_sessions(&self) -> Result<Vec<VoiceSessionsEntity>, DatabaseError> {
        let mut connection = self.pool.get().await?;
        let rows = voice_sessions::table
            .filter(voice_sessions::is_active.eq(true))
            .select(DbVoiceSession::as_select())
            .load(&mut connection)
            .await?;
        Ok(rows.into_iter().map(VoiceSessionsEntity::from).collect())
    }

    async fn find_active_sessions_by_user(
        &self,
        user_id: u64,
        guild_id: u64,
    ) -> Result<Vec<VoiceSessionsEntity>, DatabaseError> {
        let mut connection = self.pool.get().await?;
        let rows = voice_sessions::table
            .filter(voice_sessions::is_active.eq(true))
            .filter(voice_sessions::user_id.eq(user_id as i64))
            .filter(voice_sessions::guild_id.eq(guild_id as i64))
            .select(DbVoiceSession::as_select())
            .load(&mut connection)
            .await?;
        Ok(rows.into_iter().map(VoiceSessionsEntity::from).collect())
    }

    async fn get_sessions_in_range(
        &self,
        guild_id: u64,
        user_id: Option<u64>,
        since: &chrono::DateTime<chrono::Utc>,
        until: &chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<VoiceSessionsEntity>, DatabaseError> {
        let mut connection = self.pool.get().await?;
        let mut query = voice_sessions::table
            .filter(voice_sessions::guild_id.eq(guild_id as i64))
            .filter(voice_sessions::join_time.ge(since))
            .filter(voice_sessions::join_time.le(until))
            .into_boxed();
        if let Some(user_id) = user_id {
            query = query.filter(voice_sessions::user_id.eq(user_id as i64));
        }
        let rows = query
            .order(voice_sessions::join_time.asc())
            .select(DbVoiceSession::as_select())
            .load(&mut connection)
            .await?;
        Ok(rows.into_iter().map(VoiceSessionsEntity::from).collect())
    }

    async fn get_user_daily_activity(
        &self,
        user_id: u64,
        guild_id: u64,
        since: &chrono::DateTime<chrono::Utc>,
        until: &chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<VoiceDailyActivity>, DatabaseError> {
        let mut connection = self.pool.get().await?;
        let rows = diesel::sql_query(
            r#"
            SELECT DATE(join_time) AS day,
                SUM(CASE WHEN is_active
                    THEN EXTRACT(EPOCH FROM NOW())::bigint
                        - EXTRACT(EPOCH FROM join_time)::bigint
                    ELSE EXTRACT(EPOCH FROM leave_time)::bigint
                        - EXTRACT(EPOCH FROM join_time)::bigint
                END)::bigint AS total_seconds
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
        .load(&mut connection)
        .await?;
        Ok(rows)
    }

    async fn get_guild_daily_stats(
        &self,
        guild_id: u64,
        since: &chrono::DateTime<chrono::Utc>,
        until: &chrono::DateTime<chrono::Utc>,
        stat_type: GuildStatType,
    ) -> Result<Vec<GuildDailyStats>, DatabaseError> {
        let mut connection = self.pool.get().await?;
        let query = match stat_type {
            GuildStatType::ActiveUserCount => diesel::sql_query(
                r#"
                SELECT DATE(join_time) AS day, COUNT(DISTINCT user_id)::bigint AS value
                FROM voice_sessions
                WHERE guild_id = $1 AND join_time >= $2 AND join_time <= $3
                GROUP BY DATE(join_time) ORDER BY day
                "#,
            )
            .bind::<diesel::sql_types::BigInt, _>(guild_id as i64)
            .bind::<diesel::sql_types::Timestamptz, _>(since)
            .bind::<diesel::sql_types::Timestamptz, _>(until),
            GuildStatType::AverageTime => diesel::sql_query(
                r#"
                SELECT day, CAST(AVG(user_daily_total) AS BIGINT) AS value
                FROM (
                    SELECT user_id, DATE(join_time) AS day,
                        SUM(CASE WHEN is_active
                            THEN EXTRACT(EPOCH FROM NOW())::bigint
                                - EXTRACT(EPOCH FROM join_time)::bigint
                            ELSE EXTRACT(EPOCH FROM leave_time)::bigint
                        - EXTRACT(EPOCH FROM join_time)::bigint
                        END)::bigint AS user_daily_total
                    FROM voice_sessions
                    WHERE guild_id = $1 AND join_time >= $2 AND join_time <= $3
                    GROUP BY user_id, DATE(join_time)
                ) totals
                GROUP BY day ORDER BY day
                "#,
            )
            .bind::<diesel::sql_types::BigInt, _>(guild_id as i64)
            .bind::<diesel::sql_types::Timestamptz, _>(since)
            .bind::<diesel::sql_types::Timestamptz, _>(until),
            GuildStatType::TotalTime => diesel::sql_query(
                r#"
                SELECT day, SUM(user_daily_total)::bigint AS value
                FROM (
                    SELECT user_id, DATE(join_time) AS day,
                        SUM(CASE WHEN is_active
                            THEN EXTRACT(EPOCH FROM NOW())::bigint
                                - EXTRACT(EPOCH FROM join_time)::bigint
                            ELSE EXTRACT(EPOCH FROM leave_time)::bigint
                        - EXTRACT(EPOCH FROM join_time)::bigint
                        END)::bigint AS user_daily_total
                    FROM voice_sessions
                    WHERE guild_id = $1 AND join_time >= $2 AND join_time <= $3
                    GROUP BY user_id, DATE(join_time)
                ) totals
                GROUP BY day ORDER BY day
                "#,
            )
            .bind::<diesel::sql_types::BigInt, _>(guild_id as i64)
            .bind::<diesel::sql_types::Timestamptz, _>(since)
            .bind::<diesel::sql_types::Timestamptz, _>(until),
        };
        Ok(query.load(&mut connection).await?)
    }
}

#[derive(Clone)]
pub struct PgVoiceSettingsRepo {
    pool: DbPool,
}

impl PgVoiceSettingsRepo {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl VoiceSettingsRepository for PgVoiceSettingsRepo {
    async fn list_all(&self) -> Result<Vec<VoiceSettingsEntity>, DatabaseError> {
        let mut connection = self.pool.get().await?;
        Ok(voice_settings::table
            .select(VoiceSettingsEntity::as_select())
            .load(&mut connection)
            .await?)
    }

    async fn get(&self, guild_id: u64) -> Result<Option<VoiceSettingsEntity>, DatabaseError> {
        let mut connection = self.pool.get().await?;
        Ok(voice_settings::table
            .find(guild_id as i64)
            .select(VoiceSettingsEntity::as_select())
            .first(&mut connection)
            .await
            .optional()?)
    }

    async fn replace(&self, settings: &VoiceSettingsEntity) -> Result<(), DatabaseError> {
        let mut connection = self.pool.get().await?;
        diesel::insert_into(voice_settings::table)
            .values(settings)
            .on_conflict(voice_settings::guild_id)
            .do_update()
            .set(voice_settings::enabled.eq(settings.enabled))
            .execute(&mut connection)
            .await?;
        Ok(())
    }

    async fn delete_all(&self) -> Result<(), DatabaseError> {
        let mut connection = self.pool.get().await?;
        diesel::delete(voice_settings::table)
            .execute(&mut connection)
            .await?;
        Ok(())
    }
}

#[allow(clippy::redundant_field_names)]
#[derive(QueryableByName)]
struct LegacyVoiceSettingRow {
    #[diesel(sql_type = BigInt)]
    guild_id: i64,
    #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Bool>)]
    voice_enabled: Option<bool>,
}

#[allow(clippy::redundant_field_names)]
#[derive(QueryableByName)]
struct LegacyImportStateRow {
    #[diesel(sql_type = diesel::sql_types::Integer)]
    #[diesel(column_name = id)]
    _id: i32,
}

pub async fn import_legacy_settings_once(pool: &DbPool) -> Result<u64, DatabaseError> {
    let mut connection = pool.get().await?;
    let already_imported =
        diesel::sql_query("SELECT 1 AS id FROM voice_settings_import_state WHERE id = 1 LIMIT 1")
            .get_result::<LegacyImportStateRow>(&mut connection)
            .await
            .optional()?
            .is_some();
    if already_imported {
        return Ok(0);
    }

    let rows: Vec<LegacyVoiceSettingRow> = diesel::sql_query(concat!(
        "SELECT guild_id, (settings #>> '{voice,enabled}')::boolean AS voice_enabled ",
        "FROM server_settings"
    ))
    .load(&mut connection)
    .await?;
    let mut imported = 0;
    for row in rows {
        diesel::insert_into(voice_settings::table)
            .values((
                voice_settings::guild_id.eq(row.guild_id),
                voice_settings::enabled.eq(row.voice_enabled.unwrap_or(true)),
            ))
            .on_conflict(voice_settings::guild_id)
            .do_nothing()
            .execute(&mut connection)
            .await?;
        imported += 1;
    }
    diesel::sql_query("INSERT INTO voice_settings_import_state (id) VALUES (1)")
        .execute(&mut connection)
        .await?;
    Ok(imported)
}
