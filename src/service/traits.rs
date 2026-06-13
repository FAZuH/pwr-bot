//! Business logic interfaces (Services).
//!
//! Services orchestrate repositories and external platforms to implement
//! high-level business rules. They are the only layer that should handle
//! cross-entity logic and complex validations.

use std::vec::Vec;

use async_trait::async_trait;

use crate::entity::*;
use crate::repo::error::DatabaseError;
use crate::service::error::ServiceError;
use crate::service::feed_subscription::SubscribeResult;
use crate::service::feed_subscription::SubscriberTarget;
use crate::service::feed_subscription::UnsubscribeResult;
use crate::service::internal::DatabaseDump;

/// Interface for feed subscription operations.
///
/// Full implementation lives in the feed plugin. Core commands still
/// reference this trait during the transition.
#[async_trait]
pub trait FeedSubscriptionProvider: Send + Sync {
    async fn subscribe(
        &self,
        url: &str,
        subscriber: &SubscriberEntity,
    ) -> Result<SubscribeResult, ServiceError>;

    async fn unsubscribe(
        &self,
        source_url: &str,
        subscriber: &SubscriberEntity,
    ) -> Result<UnsubscribeResult, ServiceError>;

    async fn get_server_settings(&self, guild_id: u64) -> Result<ServerSettings, ServiceError>;

    async fn update_server_settings(
        &self,
        guild_id: u64,
        settings: ServerSettings,
    ) -> Result<(), ServiceError>;

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

    async fn get_or_create_subscriber(
        &self,
        target: &SubscriberTarget,
    ) -> Result<SubscriberEntity, ServiceError>;
}

/// Generic interface for managing server-wide configuration.
#[async_trait]
pub trait SettingsProvider: Send + Sync {
    async fn get_server_settings(&self, guild_id: u64) -> Result<ServerSettings, ServiceError>;

    async fn update_server_settings(
        &self,
        guild_id: u64,
        settings: ServerSettings,
    ) -> Result<(), ServiceError>;
}

/// Internal bot operations and metadata management.
#[async_trait]
pub trait InternalOps: Send + Sync {
    async fn get_meta(&self, key: BotMetaKey) -> Result<Option<String>, DatabaseError>;

    async fn set_meta(&self, key: BotMetaKey, value: String) -> Result<(), DatabaseError>;

    async fn dump_database(&self) -> anyhow::Result<DatabaseDump>;
}
