//! Traits for database operations.
//!
//! This module defines the persistence layer interfaces using the Repository pattern.
//! Each trait represents a specific table or a logical group of database operations.

use async_trait::async_trait;

use crate::entity::*;
use crate::repo::error::DatabaseError;

/// Trait for basic table maintenance operations.
#[async_trait]
pub trait TableBase: Send + Sync {
    /// Creates the table if it does not exist.
    async fn create_table(&self) -> Result<(), DatabaseError>;
    /// Deletes all rows from the table.
    async fn delete_all(&self) -> Result<(), DatabaseError>;
}

/// Generic trait for standard CRUD (Create, Read, Update, Delete) operations.
///
/// `T` is the domain entity type, and `ID` is the primary key type.
#[async_trait]
pub trait CrudTable<T, ID>: TableBase {
    /// Returns all records from the table.
    async fn select_all(&self) -> Result<Vec<T>, DatabaseError>;
    /// Inserts a new record and returns its ID.
    async fn insert(&self, model: &T) -> Result<ID, DatabaseError>;
    /// Selects a single record by its ID.
    async fn select(&self, id: &ID) -> Result<Option<T>, DatabaseError>;
    /// Updates an existing record.
    async fn update(&self, model: &T) -> Result<(), DatabaseError>;
    /// Deletes a record by its ID.
    async fn delete(&self, id: &ID) -> Result<(), DatabaseError>;
    /// Replaces an existing record or inserts a new one.
    async fn replace(&self, model: &T) -> Result<ID, DatabaseError>;
}

/// Read-only projections for the host database dump.
#[async_trait]
pub trait FeedDumpRepository: Send + Sync {
    async fn select_feeds(&self) -> Result<Vec<FeedEntity>, DatabaseError>;
    async fn select_feed_items(&self) -> Result<Vec<FeedItemEntity>, DatabaseError>;
    async fn select_subscribers(&self) -> Result<Vec<SubscriberEntity>, DatabaseError>;
    async fn select_subscriptions(&self) -> Result<Vec<FeedSubscriptionEntity>, DatabaseError>;
}

/// Operations for the `server_settings` table.
#[async_trait]
pub trait ServerSettingsRepository: CrudTable<ServerSettingsEntity, u64> + Send + Sync {}

/// Operations for tracking voice channel activity.
#[async_trait]
pub trait VoiceSessionsRepository: CrudTable<VoiceSessionsEntity, i32> + Send + Sync {
    /// Generic leaderboard query with filters.
    async fn get_leaderboard_opt(
        &self,
        opts: &VoiceLeaderboardOpt,
    ) -> Result<Vec<VoiceLeaderboardEntry>, DatabaseError>;
    /// Returns the top users by voice activity in a guild.
    async fn get_leaderboard(
        &self,
        guild_id: u64,
        limit: u32,
    ) -> Result<Vec<VoiceLeaderboardEntry>, DatabaseError>;
    /// Paginated leaderboard query.
    async fn get_leaderboard_with_offset(
        &self,
        guild_id: u64,
        offset: u32,
        limit: u32,
    ) -> Result<Vec<VoiceLeaderboardEntry>, DatabaseError>;
    /// Returns a leaderboard of users who spent the most time in VCs with a target user.
    async fn get_partner_leaderboard(
        &self,
        opts: &VoiceLeaderboardOpt,
        target_user_id: u64,
    ) -> Result<Vec<VoiceLeaderboardEntry>, DatabaseError>;
    /// Updates the end time for an active voice session.
    async fn update_leave_time(
        &self,
        user_id: u64,
        channel_id: u64,
        join_time: &chrono::DateTime<chrono::Utc>,
        leave_time: &chrono::DateTime<chrono::Utc>,
    ) -> Result<(), DatabaseError>;
    /// Marks a session as closed.
    async fn close_session(
        &self,
        user_id: u64,
        channel_id: u64,
        join_time: &chrono::DateTime<chrono::Utc>,
        leave_time: &chrono::DateTime<chrono::Utc>,
    ) -> Result<(), DatabaseError>;
    /// Returns all sessions currently marked as active.
    async fn find_active_sessions(&self) -> Result<Vec<VoiceSessionsEntity>, DatabaseError>;
    /// Returns all active sessions for a specific user in a guild.
    async fn find_active_sessions_by_user(
        &self,
        user_id: u64,
        guild_id: u64,
    ) -> Result<Vec<VoiceSessionsEntity>, DatabaseError>;
    /// Returns all sessions within a specific time range.
    async fn get_sessions_in_range(
        &self,
        guild_id: u64,
        user_id: Option<u64>,
        since: &chrono::DateTime<chrono::Utc>,
        until: &chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<VoiceSessionsEntity>, DatabaseError>;
    /// Aggregates daily activity for a specific user.
    async fn get_user_daily_activity(
        &self,
        user_id: u64,
        guild_id: u64,
        since: &chrono::DateTime<chrono::Utc>,
        until: &chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<VoiceDailyActivity>, DatabaseError>;
    /// Aggregates daily total voice time for a guild.
    async fn get_guild_daily_total_time(
        &self,
        guild_id: u64,
        since: &chrono::DateTime<chrono::Utc>,
        until: &chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<GuildDailyStats>, DatabaseError>;
    /// Aggregates daily average voice time per user for a guild.
    async fn get_guild_daily_average_time(
        &self,
        guild_id: u64,
        since: &chrono::DateTime<chrono::Utc>,
        until: &chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<GuildDailyStats>, DatabaseError>;
    /// Aggregates daily unique user count in VCs for a guild.
    async fn get_guild_daily_user_count(
        &self,
        guild_id: u64,
        since: &chrono::DateTime<chrono::Utc>,
        until: &chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<GuildDailyStats>, DatabaseError>;
}

/// Operations for internal bot metadata.
#[async_trait]
pub trait BotMetaRepository: CrudTable<BotMetaEntity, String> + Send + Sync {
    /// Checks if the metadata table exists.
    async fn table_exists(&self) -> bool;
}

/// Operations for the `plugin_kv` table: per-namespace string key-value
/// storage served to plugins via `host.kv.*`. The composite primary key
/// (namespace, key) does not fit [`CrudTable`].
#[async_trait]
pub trait PluginKvRepository: TableBase + Send + Sync {
    /// Returns the value for a key in a namespace, or `None` when unset.
    async fn get(&self, namespace: &str, key: &str) -> Result<Option<String>, DatabaseError>;
    /// Upserts the value for a key in a namespace.
    async fn set(&self, namespace: &str, key: &str, value: &str) -> Result<(), DatabaseError>;
    /// Deletes a key in a namespace. No-op when the key is absent.
    async fn delete(&self, namespace: &str, key: &str) -> Result<(), DatabaseError>;
}

/// Operations for the `guild_plugins` table: per-guild plugin enable/disable
/// state. The composite primary key (guild_id, plugin_name) does not fit
/// [`CrudTable`].
#[async_trait]
pub trait GuildPluginRepository: TableBase + Send + Sync {
    /// Returns the enable/disable state of every plugin in a guild.
    async fn list_for_guild(&self, guild_id: u64) -> Result<Vec<GuildPluginEntity>, DatabaseError>;
    /// Upserts the enable/disable state of a plugin in a guild.
    async fn set_enabled(
        &self,
        guild_id: u64,
        plugin_name: &str,
        enabled: bool,
    ) -> Result<(), DatabaseError>;
    /// Deletes the state row for a plugin in a guild. No-op when absent.
    async fn delete(&self, guild_id: u64, plugin_name: &str) -> Result<(), DatabaseError>;
}

/// Factory trait providing access to individual repository handles.
///
/// Each method clones the underlying pool-backed handle and returns
/// a boxed trait object. Call at service construction time, not per-operation.
pub trait Repos: Send + Sync {
    fn feed_dump(&self) -> Box<dyn FeedDumpRepository + Send + Sync>;
    fn server_settings(&self) -> Box<dyn ServerSettingsRepository + Send + Sync>;
    fn voice_sessions(&self) -> Box<dyn VoiceSessionsRepository + Send + Sync>;
    fn bot_meta(&self) -> Box<dyn BotMetaRepository + Send + Sync>;
    fn plugin_kv(&self) -> Box<dyn PluginKvRepository + Send + Sync>;
    fn guild_plugins(&self) -> Box<dyn GuildPluginRepository + Send + Sync>;
}
