use std::vec::Vec;

use async_trait::async_trait;
use chrono::DateTime;
use chrono::Utc;

use crate::bot::commands::voice::GuildStatType;
use crate::entity::*;
use crate::repository::error::DatabaseError;
use crate::service::error::ServiceError;
use crate::service::feed_subscription_service::FeedUpdateResult;
use crate::service::feed_subscription_service::SubscribeResult;
use crate::service::feed_subscription_service::Subscription;
use crate::service::feed_subscription_service::UnsubscribeResult;

#[async_trait]
pub trait FeedSubscriptionProvider: Send + Sync {
    async fn subscribe(
        &self,
        url: &str,
        subscriber: &SubscriberEntity,
    ) -> Result<SubscribeResult, ServiceError>;
    async fn get_feeds_by_tag(&self, tag: &str) -> Result<Vec<FeedEntity>, ServiceError>;
    async fn get_both_subscribers(
        &self,
        target_id: String,
        guild_id: Option<String>,
    ) -> (Option<SubscriberEntity>, Option<SubscriberEntity>);
    async fn search_and_combine_feeds(
        &self,
        partial: &str,
        user_subscriber: Option<SubscriberEntity>,
        guild_subscriber: Option<SubscriberEntity>,
    ) -> Vec<FeedEntity>;
    async fn check_feed_update(&self, feed: &FeedEntity) -> Result<FeedUpdateResult, ServiceError>;
    async fn unsubscribe(
        &self,
        source_url: &str,
        subscriber: &SubscriberEntity,
    ) -> Result<UnsubscribeResult, ServiceError>;
    async fn list_paginated_subscriptions(
        &self,
        subscriber: &SubscriberEntity,
        page: u32,
        per_page: u32,
    ) -> Result<Vec<Subscription>, ServiceError>;
    async fn get_subscription_count(
        &self,
        subscriber: &SubscriberEntity,
    ) -> Result<u32, ServiceError>;
    async fn search_subcriptions(
        &self,
        subscriber: &SubscriberEntity,
        partial: &str,
    ) -> Result<Vec<FeedEntity>, ServiceError>;
    async fn get_or_create_feed(&self, source_url: &str) -> Result<FeedEntity, ServiceError>;
    async fn get_or_create_subscriber(
        &self,
        target: &crate::service::feed_subscription_service::SubscriberTarget,
    ) -> Result<SubscriberEntity, ServiceError>;
    async fn get_feed_by_source_url(
        &self,
        source_url: &str,
    ) -> Result<Option<FeedEntity>, ServiceError>;
    async fn get_server_settings(&self, guild_id: u64) -> Result<ServerSettings, ServiceError>;
    async fn update_server_settings(
        &self,
        guild_id: u64,
        settings: ServerSettings,
    ) -> Result<(), ServiceError>;
}

#[async_trait]
pub trait VoiceTracker: Send + Sync {
    async fn is_enabled(&self, guild_id: u64) -> bool;
    async fn insert(&self, model: &VoiceSessionsEntity) -> anyhow::Result<()>;
    async fn replace(&self, model: &VoiceSessionsEntity) -> anyhow::Result<()>;
    async fn get_server_settings(&self, guild_id: u64) -> anyhow::Result<ServerSettings>;
    async fn update_server_settings(
        &self,
        guild_id: u64,
        settings: ServerSettings,
    ) -> anyhow::Result<()>;
    async fn get_leaderboard_withopt(
        &self,
        options: &VoiceLeaderboardOpt,
    ) -> anyhow::Result<Vec<VoiceLeaderboardEntry>>;
    async fn get_partner_leaderboard(
        &self,
        options: &VoiceLeaderboardOpt,
        target_user_id: u64,
    ) -> anyhow::Result<Vec<VoiceLeaderboardEntry>>;
    async fn get_leaderboard(
        &self,
        guild_id: u64,
        limit: u32,
    ) -> anyhow::Result<Vec<VoiceLeaderboardEntry>>;
    async fn get_leaderboard_with_offset(
        &self,
        guild_id: u64,
        offset: u32,
        limit: u32,
    ) -> anyhow::Result<Vec<VoiceLeaderboardEntry>>;
    async fn update_session_leave_time(
        &self,
        user_id: u64,
        channel_id: u64,
        join_time: &DateTime<Utc>,
        leave_time: &DateTime<Utc>,
    ) -> anyhow::Result<()>;
    async fn close_session(
        &self,
        user_id: u64,
        channel_id: u64,
        join_time: &DateTime<Utc>,
        leave_time: &DateTime<Utc>,
    ) -> anyhow::Result<()>;
    async fn find_active_sessions(&self) -> anyhow::Result<Vec<VoiceSessionsEntity>>;
    async fn get_sessions_in_range(
        &self,
        guild_id: u64,
        user_id: Option<u64>,
        since: &DateTime<Utc>,
        until: &DateTime<Utc>,
    ) -> anyhow::Result<Vec<VoiceSessionsEntity>>;
    async fn get_user_daily_activity(
        &self,
        user_id: u64,
        guild_id: u64,
        since: &DateTime<Utc>,
        until: &DateTime<Utc>,
    ) -> anyhow::Result<Vec<VoiceDailyActivity>>;
    async fn get_guild_daily_stats(
        &self,
        guild_id: u64,
        since: &DateTime<Utc>,
        until: &DateTime<Utc>,
        stat_type: GuildStatType,
    ) -> anyhow::Result<Vec<GuildDailyStats>>;
}

#[async_trait]
pub trait SettingsProvider: Send + Sync {
    async fn get_server_settings(&self, guild_id: u64) -> Result<ServerSettings, ServiceError>;
    async fn update_server_settings(
        &self,
        guild_id: u64,
        settings: ServerSettings,
    ) -> Result<(), ServiceError>;
}

#[async_trait]
pub trait InternalOps: Send + Sync {
    async fn get_meta(&self, key: BotMetaKey) -> Result<Option<String>, DatabaseError>;
    async fn set_meta(&self, key: BotMetaKey, value: String) -> Result<(), DatabaseError>;
    async fn dump_database(&self)
    -> anyhow::Result<crate::service::internal_service::DatabaseDump>;
}
