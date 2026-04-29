//! Business logic interfaces (Services).
//!
//! Services orchestrate repositories and external platforms to implement
//! high-level business rules. They are the only layer that should handle
//! cross-entity logic and complex validations.

use std::vec::Vec;

use async_trait::async_trait;
use chrono::DateTime;
use chrono::Utc;

use crate::bot::command::voice::GuildStatType;
use crate::entity::*;
use crate::repo::error::DatabaseError;
use crate::service::error::ServiceError;
use crate::service::feed_subscription::FeedUpdateResult;
use crate::service::feed_subscription::SubscribeResult;
use crate::service::feed_subscription::SubscriberTarget;
use crate::service::feed_subscription::Subscription;
use crate::service::feed_subscription::UnsubscribeResult;
use crate::service::internal::DatabaseDump;

/// Logic for managing feed subscriptions (AniList, MangaDex, Comick).
#[async_trait]
pub trait FeedSubscriptionProvider: Send + Sync {
    /// Subscribes a user or guild to a feed by its URL.
    async fn subscribe(
        &self,
        url: &str,
        subscriber: &SubscriberEntity,
    ) -> Result<SubscribeResult, ServiceError>;

    /// Returns all feeds tagged with a specific label.
    async fn get_feeds_by_tag(&self, tag: &str) -> Result<Vec<FeedEntity>, ServiceError>;

    /// Retrieves both User (DM) and Guild subscribers for a target ID.
    async fn get_both_subscribers(
        &self,
        target_id: String,
        guild_id: Option<String>,
    ) -> (Option<SubscriberEntity>, Option<SubscriberEntity>);

    /// Searches for feeds by name and filters by a subscriber's current follows.
    async fn search_and_combine_feeds(
        &self,
        partial: &str,
        user_subscriber: Option<SubscriberEntity>,
        guild_subscriber: Option<SubscriberEntity>,
    ) -> Vec<FeedEntity>;

    /// Polls a platform for the latest item of a feed and updates the database.
    async fn check_feed_update(&self, feed: &FeedEntity) -> Result<FeedUpdateResult, ServiceError>;

    /// Unsubscribes a user or guild from a feed.
    async fn unsubscribe(
        &self,
        source_url: &str,
        subscriber: &SubscriberEntity,
    ) -> Result<UnsubscribeResult, ServiceError>;

    /// Returns a paginated list of a subscriber's active subscriptions.
    async fn list_paginated_subscriptions(
        &self,
        subscriber: &SubscriberEntity,
        page: u32,
        per_page: u32,
    ) -> Result<Vec<Subscription>, ServiceError>;

    /// Returns the total number of subscriptions for a subscriber.
    async fn get_subscription_count(
        &self,
        subscriber: &SubscriberEntity,
    ) -> Result<u32, ServiceError>;

    /// Searches for feeds within a subscriber's active subscriptions.
    async fn search_subcriptions(
        &self,
        subscriber: &SubscriberEntity,
        partial: &str,
    ) -> Result<Vec<FeedEntity>, ServiceError>;

    /// Finds an existing feed or creates it if it doesn't exist.
    async fn get_or_create_feed(&self, source_url: &str) -> Result<FeedEntity, ServiceError>;

    /// Finds an existing subscriber (Guild/DM) or creates it.
    async fn get_or_create_subscriber(
        &self,
        target: &SubscriberTarget,
    ) -> Result<SubscriberEntity, ServiceError>;

    /// Finds a feed by its source URL.
    async fn get_feed_by_source_url(
        &self,
        source_url: &str,
    ) -> Result<Option<FeedEntity>, ServiceError>;

    /// Returns the feed-specific settings for a guild.
    async fn get_server_settings(&self, guild_id: u64) -> Result<ServerSettings, ServiceError>;

    /// Returns all subscribers of a specific type that are following a feed.
    async fn get_subscribers_by_type_and_feed(
        &self,
        subscriber_type: SubscriberType,
        feed_id: i32,
    ) -> Result<Vec<SubscriberEntity>, ServiceError>;

    /// Updates the feed settings for a guild.
    async fn update_server_settings(
        &self,
        guild_id: u64,
        settings: ServerSettings,
    ) -> Result<(), ServiceError>;
}

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
