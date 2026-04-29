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
    /// Drops the table. Use with extreme caution.
    async fn drop_table(&self) -> Result<(), DatabaseError>;
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

/// Operations for the `feed` table.
#[async_trait]
pub trait FeedRepository: CrudTable<FeedEntity, i32> + Send + Sync {
    /// Returns all feeds associated with a specific tag.
    async fn select_all_by_tag(&self, tag: &str) -> Result<Vec<FeedEntity>, DatabaseError>;
    /// Finds a feed by its platform-specific source ID.
    async fn select_by_source_id(
        &self,
        platform_id: &str,
        source_id: &str,
    ) -> Result<Option<FeedEntity>, DatabaseError>;
    /// Searches for feeds by name within a subscriber's subscriptions.
    async fn select_by_name_and_subscriber_id(
        &self,
        subscriber_id: &i32,
        name_search: &str,
        limit: Option<u32>,
    ) -> Result<Vec<FeedEntity>, DatabaseError>;
}

/// Operations for the `feed_item` table.
#[async_trait]
pub trait FeedItemRepository: CrudTable<FeedItemEntity, i32> + Send + Sync {
    /// Returns the most recently published item for a feed.
    async fn select_latest_by_feed_id(
        &self,
        feed_id: i32,
    ) -> Result<Option<FeedItemEntity>, DatabaseError>;
    /// Returns all items for a specific feed.
    async fn select_all_by_feed_id(
        &self,
        feed_id: i32,
    ) -> Result<Vec<FeedItemEntity>, DatabaseError>;
    /// Deletes all items associated with a feed.
    async fn delete_all_by_feed_id(&self, feed_id: i32) -> Result<(), DatabaseError>;
}

/// Operations for the `subscriber` table (Guilds or DMs).
#[async_trait]
pub trait SubscriberRepository: CrudTable<SubscriberEntity, i32> + Send + Sync {
    /// Returns all subscribers of a specific type that are subscribed to a feed.
    async fn select_all_by_type_and_feed(
        &self,
        r#type: SubscriberType,
        feed_id: i32,
    ) -> Result<Vec<SubscriberEntity>, DatabaseError>;
    /// Finds a subscriber by its type and Discord target ID (Guild ID or User ID).
    async fn select_by_type_and_target(
        &self,
        r#type: &SubscriberType,
        target_id: &str,
    ) -> Result<Option<SubscriberEntity>, DatabaseError>;
}

/// Operations for the `feed_subscription` table.
#[async_trait]
pub trait FeedSubscriptionRepository: CrudTable<FeedSubscriptionEntity, i32> + Send + Sync {
    /// Returns all subscriptions for a specific feed.
    async fn select_all_by_feed_id(
        &self,
        feed_id: i32,
    ) -> Result<Vec<FeedSubscriptionEntity>, DatabaseError>;
    /// Returns all subscriptions for a specific subscriber.
    async fn select_all_by_subscriber_id(
        &self,
        subscriber_id: i32,
    ) -> Result<Vec<FeedSubscriptionEntity>, DatabaseError>;
    /// Counts total subscriptions for a subscriber.
    async fn count_by_subscriber_id(&self, subscriber_id: i32) -> Result<u32, DatabaseError>;
    /// Returns a paginated list of subscriptions.
    async fn select_paginated_by_subscriber_id(
        &self,
        subscriber_id: i32,
        page: u32,
        per_page: u32,
    ) -> Result<Vec<FeedSubscriptionEntity>, DatabaseError>;
    /// Returns a paginated list of subscriptions including the latest item for each feed.
    async fn select_paginated_with_latest_by_subscriber_id(
        &self,
        subscriber_id: i32,
        page: u32,
        per_page: u32,
    ) -> Result<Vec<FeedWithLatestItemRow>, DatabaseError>;
    /// Checks if any subscriber is currently following a feed.
    async fn exists_by_feed_id(&self, feed_id: i32) -> Result<bool, DatabaseError>;
    /// Deletes a specific subscription link.
    async fn delete_subscription(
        &self,
        feed_id: i32,
        subscriber_id: i32,
    ) -> Result<bool, DatabaseError>;
    /// Deletes all subscriptions for a specific feed.
    async fn delete_all_by_feed_id(&self, feed_id: i32) -> Result<(), DatabaseError>;
    /// Deletes all subscriptions for a specific subscriber.
    async fn delete_all_by_subscriber_id(&self, subscriber_id: i32) -> Result<(), DatabaseError>;
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
