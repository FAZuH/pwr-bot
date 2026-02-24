//! Traits for database operations

use async_trait::async_trait;

use crate::entity::*;
use crate::repository::error::DatabaseError;

/// Trait for basic table operations.
#[async_trait]
pub trait TableBase: Send + Sync {
    async fn create_table(&self) -> Result<(), DatabaseError>;
    async fn drop_table(&self) -> Result<(), DatabaseError>;
    async fn delete_all(&self) -> Result<(), DatabaseError>;
}

/// Generic trait for CRUD operations.
#[async_trait]
pub trait CrudTable<T, ID>: TableBase {
    async fn select_all(&self) -> Result<Vec<T>, DatabaseError>;
    async fn insert(&self, model: &T) -> Result<ID, DatabaseError>;
    async fn select(&self, id: &ID) -> Result<Option<T>, DatabaseError>;
    async fn update(&self, model: &T) -> Result<(), DatabaseError>;
    async fn delete(&self, id: &ID) -> Result<(), DatabaseError>;
    async fn replace(&self, model: &T) -> Result<ID, DatabaseError>;
}

#[async_trait]
pub trait FeedRepository: CrudTable<FeedEntity, i32> + Send + Sync {
    async fn select_all_by_tag(&self, tag: &str) -> Result<Vec<FeedEntity>, DatabaseError>;
    async fn select_by_source_id(
        &self,
        platform_id: &str,
        source_id: &str,
    ) -> Result<Option<FeedEntity>, DatabaseError>;
    async fn select_by_name_and_subscriber_id(
        &self,
        subscriber_id: &i32,
        name_search: &str,
        limit: Option<u32>,
    ) -> Result<Vec<FeedEntity>, DatabaseError>;
}

#[async_trait]
pub trait FeedItemRepository: CrudTable<FeedItemEntity, i32> + Send + Sync {
    async fn select_latest_by_feed_id(
        &self,
        feed_id: i32,
    ) -> Result<Option<FeedItemEntity>, DatabaseError>;
    async fn select_all_by_feed_id(
        &self,
        feed_id: i32,
    ) -> Result<Vec<FeedItemEntity>, DatabaseError>;
    async fn delete_all_by_feed_id(&self, feed_id: i32) -> Result<(), DatabaseError>;
}

#[async_trait]
pub trait SubscriberRepository: CrudTable<SubscriberEntity, i32> + Send + Sync {
    async fn select_all_by_type_and_feed(
        &self,
        r#type: SubscriberType,
        feed_id: i32,
    ) -> Result<Vec<SubscriberEntity>, DatabaseError>;
    async fn select_by_type_and_target(
        &self,
        r#type: &SubscriberType,
        target_id: &str,
    ) -> Result<Option<SubscriberEntity>, DatabaseError>;
}

#[async_trait]
pub trait FeedSubscriptionRepository: CrudTable<FeedSubscriptionEntity, i32> + Send + Sync {
    async fn select_all_by_feed_id(
        &self,
        feed_id: i32,
    ) -> Result<Vec<FeedSubscriptionEntity>, DatabaseError>;
    async fn select_all_by_subscriber_id(
        &self,
        subscriber_id: i32,
    ) -> Result<Vec<FeedSubscriptionEntity>, DatabaseError>;
    async fn count_by_subscriber_id(&self, subscriber_id: i32) -> Result<u32, DatabaseError>;
    async fn select_paginated_by_subscriber_id(
        &self,
        subscriber_id: i32,
        page: u32,
        per_page: u32,
    ) -> Result<Vec<FeedSubscriptionEntity>, DatabaseError>;
    async fn select_paginated_with_latest_by_subscriber_id(
        &self,
        subscriber_id: i32,
        page: u32,
        per_page: u32,
    ) -> Result<Vec<FeedWithLatestItemRow>, DatabaseError>;
    async fn exists_by_feed_id(&self, feed_id: i32) -> Result<bool, DatabaseError>;
    async fn delete_subscription(
        &self,
        feed_id: i32,
        subscriber_id: i32,
    ) -> Result<bool, DatabaseError>;
    async fn delete_all_by_feed_id(&self, feed_id: i32) -> Result<(), DatabaseError>;
    async fn delete_all_by_subscriber_id(&self, subscriber_id: i32) -> Result<(), DatabaseError>;
}

#[async_trait]
pub trait ServerSettingsRepository: CrudTable<ServerSettingsEntity, u64> + Send + Sync {}

#[async_trait]
pub trait VoiceSessionsRepository: CrudTable<VoiceSessionsEntity, i32> + Send + Sync {
    async fn get_leaderboard_opt(
        &self,
        opts: &VoiceLeaderboardOpt,
    ) -> Result<Vec<VoiceLeaderboardEntry>, DatabaseError>;
    async fn get_leaderboard(
        &self,
        guild_id: u64,
        limit: u32,
    ) -> Result<Vec<VoiceLeaderboardEntry>, DatabaseError>;
    async fn get_leaderboard_with_offset(
        &self,
        guild_id: u64,
        offset: u32,
        limit: u32,
    ) -> Result<Vec<VoiceLeaderboardEntry>, DatabaseError>;
    async fn get_partner_leaderboard(
        &self,
        opts: &VoiceLeaderboardOpt,
        target_user_id: u64,
    ) -> Result<Vec<VoiceLeaderboardEntry>, DatabaseError>;
    async fn update_leave_time(
        &self,
        user_id: u64,
        channel_id: u64,
        join_time: &chrono::DateTime<chrono::Utc>,
        leave_time: &chrono::DateTime<chrono::Utc>,
    ) -> Result<(), DatabaseError>;
    async fn close_session(
        &self,
        user_id: u64,
        channel_id: u64,
        join_time: &chrono::DateTime<chrono::Utc>,
        leave_time: &chrono::DateTime<chrono::Utc>,
    ) -> Result<(), DatabaseError>;
    async fn find_active_sessions(&self) -> Result<Vec<VoiceSessionsEntity>, DatabaseError>;
    async fn get_sessions_in_range(
        &self,
        guild_id: u64,
        user_id: Option<u64>,
        since: &chrono::DateTime<chrono::Utc>,
        until: &chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<VoiceSessionsEntity>, DatabaseError>;
    async fn get_user_daily_activity(
        &self,
        user_id: u64,
        guild_id: u64,
        since: &chrono::DateTime<chrono::Utc>,
        until: &chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<VoiceDailyActivity>, DatabaseError>;
    async fn get_guild_daily_total_time(
        &self,
        guild_id: u64,
        since: &chrono::DateTime<chrono::Utc>,
        until: &chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<GuildDailyStats>, DatabaseError>;
    async fn get_guild_daily_average_time(
        &self,
        guild_id: u64,
        since: &chrono::DateTime<chrono::Utc>,
        until: &chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<GuildDailyStats>, DatabaseError>;
    async fn get_guild_daily_user_count(
        &self,
        guild_id: u64,
        since: &chrono::DateTime<chrono::Utc>,
        until: &chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<GuildDailyStats>, DatabaseError>;
}

#[async_trait]
pub trait BotMetaRepository: CrudTable<BotMetaEntity, String> + Send + Sync {
    async fn table_exists(&self) -> bool;
}
