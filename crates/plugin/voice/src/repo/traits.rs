use async_trait::async_trait;
use chrono::DateTime;
use chrono::Utc;

use crate::GuildDailyStats;
use crate::GuildStatType;
use crate::VoiceDailyActivity;
use crate::VoiceLeaderboardEntry;
use crate::VoiceLeaderboardOpt;
use crate::VoiceSessionsEntity;
use crate::repo::error::DatabaseError;

#[async_trait]
pub trait VoiceSessionsRepository: Send + Sync {
    async fn select_all(&self) -> Result<Vec<VoiceSessionsEntity>, DatabaseError>;
    async fn insert(&self, model: &VoiceSessionsEntity) -> Result<i32, DatabaseError>;
    async fn replace(&self, model: &VoiceSessionsEntity) -> Result<i32, DatabaseError>;
    async fn delete_all(&self) -> Result<(), DatabaseError>;
    async fn get_leaderboard_opt(
        &self,
        options: &VoiceLeaderboardOpt,
    ) -> Result<Vec<VoiceLeaderboardEntry>, DatabaseError>;
    async fn get_partner_leaderboard(
        &self,
        options: &VoiceLeaderboardOpt,
        target_user_id: u64,
    ) -> Result<Vec<VoiceLeaderboardEntry>, DatabaseError>;
    async fn update_leave_time(
        &self,
        user_id: u64,
        channel_id: u64,
        join_time: &DateTime<Utc>,
        leave_time: &DateTime<Utc>,
    ) -> Result<(), DatabaseError>;
    async fn close_session(
        &self,
        user_id: u64,
        channel_id: u64,
        join_time: &DateTime<Utc>,
        leave_time: &DateTime<Utc>,
    ) -> Result<(), DatabaseError>;
    async fn find_active_sessions(&self) -> Result<Vec<VoiceSessionsEntity>, DatabaseError>;
    async fn find_active_sessions_by_user(
        &self,
        user_id: u64,
        guild_id: u64,
    ) -> Result<Vec<VoiceSessionsEntity>, DatabaseError>;
    async fn get_sessions_in_range(
        &self,
        guild_id: u64,
        user_id: Option<u64>,
        since: &DateTime<Utc>,
        until: &DateTime<Utc>,
    ) -> Result<Vec<VoiceSessionsEntity>, DatabaseError>;
    async fn get_user_daily_activity(
        &self,
        user_id: u64,
        guild_id: u64,
        since: &DateTime<Utc>,
        until: &DateTime<Utc>,
    ) -> Result<Vec<VoiceDailyActivity>, DatabaseError>;
    async fn get_guild_daily_stats(
        &self,
        guild_id: u64,
        since: &DateTime<Utc>,
        until: &DateTime<Utc>,
        stat_type: GuildStatType,
    ) -> Result<Vec<GuildDailyStats>, DatabaseError>;
}

#[async_trait]
pub trait VoiceSettingsRepository: Send + Sync {
    async fn list_all(&self) -> Result<Vec<crate::VoiceSettingsEntity>, DatabaseError>;
    async fn get(&self, guild_id: u64)
    -> Result<Option<crate::VoiceSettingsEntity>, DatabaseError>;
    async fn replace(&self, settings: &crate::VoiceSettingsEntity) -> Result<(), DatabaseError>;
    async fn delete_all(&self) -> Result<(), DatabaseError>;
}
