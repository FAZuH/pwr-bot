//! Business logic interfaces (Services).
//!
//! Services orchestrate repositories and external platforms to implement
//! high-level business rules. They are the only layer that should handle
//! cross-entity logic and complex validations.

use std::vec::Vec;

use async_trait::async_trait;
use chrono::DateTime;
use chrono::Utc;
use mockall::automock;

use crate::bot::command::voice::GuildStatType;
use crate::entity::*;
use crate::repo::error::DatabaseError;
use crate::service::error::ServiceError;
use crate::service::internal::DatabaseDump;

/// Logic for tracking and querying voice channel activity.
#[async_trait]
pub trait VoiceTracker: Send + Sync {
    /// Checks if voice tracking is enabled for a specific guild.
    async fn is_enabled(&self, guild_id: u64) -> bool;

    /// Logs a voice session start.
    async fn insert(&self, model: &VoiceSessionsEntity) -> anyhow::Result<()>;

    /// Updates or replaces an existing voice session.
    async fn replace(&self, model: &VoiceSessionsEntity) -> anyhow::Result<()>;

    /// Returns the voice-specific settings for a guild.
    async fn get_server_settings(&self, guild_id: u64) -> anyhow::Result<ServerSettings>;

    /// Updates the voice settings for a guild.
    async fn update_server_settings(
        &self,
        guild_id: u64,
        settings: ServerSettings,
    ) -> anyhow::Result<()>;

    /// Returns a leaderboard using custom filter options.
    async fn get_leaderboard_withopt(
        &self,
        options: &VoiceLeaderboardOpt,
    ) -> anyhow::Result<Vec<VoiceLeaderboardEntry>>;

    /// Returns a leaderboard of users who spent the most time in VCs with a target user.
    async fn get_partner_leaderboard(
        &self,
        options: &VoiceLeaderboardOpt,
        target_user_id: u64,
    ) -> anyhow::Result<Vec<VoiceLeaderboardEntry>>;

    /// Returns the top users by voice time in a guild.
    async fn get_leaderboard(
        &self,
        guild_id: u64,
        limit: u32,
    ) -> anyhow::Result<Vec<VoiceLeaderboardEntry>>;

    /// Paginated leaderboard query.
    async fn get_leaderboard_with_offset(
        &self,
        guild_id: u64,
        offset: u32,
        limit: u32,
    ) -> anyhow::Result<Vec<VoiceLeaderboardEntry>>;

    /// Updates the end time for a voice session.
    async fn update_session_leave_time(
        &self,
        user_id: u64,
        channel_id: u64,
        join_time: &DateTime<Utc>,
        leave_time: &DateTime<Utc>,
    ) -> anyhow::Result<()>;

    /// Closes a voice session.
    async fn close_session(
        &self,
        user_id: u64,
        channel_id: u64,
        join_time: &DateTime<Utc>,
        leave_time: &DateTime<Utc>,
    ) -> anyhow::Result<()>;

    /// Returns all active voice sessions.
    async fn find_active_sessions(&self) -> anyhow::Result<Vec<VoiceSessionsEntity>>;

    /// Returns all active voice sessions for a specific user in a guild.
    async fn find_active_sessions_by_user(
        &self,
        user_id: u64,
        guild_id: u64,
    ) -> anyhow::Result<Vec<VoiceSessionsEntity>>;

    /// Returns all voice sessions within a time range.
    async fn get_sessions_in_range(
        &self,
        guild_id: u64,
        user_id: Option<u64>,
        since: &DateTime<Utc>,
        until: &DateTime<Utc>,
    ) -> anyhow::Result<Vec<VoiceSessionsEntity>>;

    /// Aggregates daily activity for a user.
    async fn get_user_daily_activity(
        &self,
        user_id: u64,
        guild_id: u64,
        since: &DateTime<Utc>,
        until: &DateTime<Utc>,
    ) -> anyhow::Result<Vec<VoiceDailyActivity>>;

    /// Aggregates daily stats (Total time, Average, User count) for a guild.
    async fn get_guild_daily_stats(
        &self,
        guild_id: u64,
        since: &DateTime<Utc>,
        until: &DateTime<Utc>,
        stat_type: GuildStatType,
    ) -> anyhow::Result<Vec<GuildDailyStats>>;
}

/// Generic interface for managing server-wide configuration.
#[automock]
#[async_trait]
pub trait SettingsProvider: Send + Sync {
    /// Returns all settings for a guild.
    async fn get_server_settings(&self, guild_id: u64) -> Result<ServerSettings, ServiceError>;

    /// Updates settings for a guild.
    async fn update_server_settings(
        &self,
        guild_id: u64,
        settings: ServerSettings,
    ) -> Result<(), ServiceError>;
}

/// Internal bot operations and metadata management.
#[async_trait]
pub trait InternalOps: Send + Sync {
    /// Retrieves a piece of metadata by key.
    async fn get_meta(&self, key: BotMetaKey) -> Result<Option<String>, DatabaseError>;

    /// Stores a piece of metadata.
    async fn set_meta(&self, key: BotMetaKey, value: String) -> Result<(), DatabaseError>;

    /// Generates a complete database dump as a string.
    async fn dump_database(&self) -> anyhow::Result<DatabaseDump>;
}
