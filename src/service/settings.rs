//! Server settings service for centralized settings management.

use std::sync::Arc;

use crate::entity::FeedEntity;
use crate::entity::Json;
use crate::entity::ServerSettings;
use crate::entity::ServerSettingsEntity;
use crate::entity::SubscriberEntity;
use crate::repo::traits::*;
use crate::service::error::ServiceError;
use crate::service::feed_subscription::SubscribeResult;
use crate::service::feed_subscription::SubscriberTarget;
use crate::service::feed_subscription::UnsubscribeResult;
use crate::service::traits::FeedSubscriptionProvider;
use crate::service::traits::SettingsProvider;

#[async_trait::async_trait]
impl FeedSubscriptionProvider for SettingsService {
    async fn subscribe(
        &self,
        _url: &str,
        _subscriber: &SubscriberEntity,
    ) -> Result<SubscribeResult, ServiceError> {
        Err(ServiceError::UnexpectedResult {
            message: "subscribe: feed plugin not initialized".to_string(),
        })
    }

    async fn unsubscribe(
        &self,
        _source_url: &str,
        _subscriber: &SubscriberEntity,
    ) -> Result<UnsubscribeResult, ServiceError> {
        Err(ServiceError::UnexpectedResult {
            message: "unsubscribe: feed plugin not initialized".to_string(),
        })
    }

    async fn get_server_settings(&self, guild_id: u64) -> Result<ServerSettings, ServiceError> {
        self.get_server_settings(guild_id).await
    }

    async fn update_server_settings(
        &self,
        guild_id: u64,
        settings: ServerSettings,
    ) -> Result<(), ServiceError> {
        self.update_server_settings(guild_id, settings).await
    }

    async fn get_both_subscribers(
        &self,
        _target_id: String,
        _guild_id: Option<String>,
    ) -> (Option<SubscriberEntity>, Option<SubscriberEntity>) {
        (None, None)
    }

    async fn search_and_combine_feeds(
        &self,
        _partial: &str,
        _user_subscriber: Option<SubscriberEntity>,
        _guild_subscriber: Option<SubscriberEntity>,
    ) -> Vec<FeedEntity> {
        vec![]
    }

    async fn get_or_create_subscriber(
        &self,
        _target: &SubscriberTarget,
    ) -> Result<SubscriberEntity, ServiceError> {
        Err(ServiceError::UnexpectedResult {
            message: "get_or_create_subscriber: feed plugin not initialized".to_string(),
        })
    }
}

#[async_trait::async_trait]
impl SettingsProvider for SettingsService {
    async fn get_server_settings(&self, guild_id: u64) -> Result<ServerSettings, ServiceError> {
        self.get_server_settings(guild_id).await
    }

    async fn update_server_settings(
        &self,
        guild_id: u64,
        settings: ServerSettings,
    ) -> Result<(), ServiceError> {
        self.update_server_settings(guild_id, settings).await
    }
}

/// Service for managing server settings.
/// Provides a single source of truth for all server configuration.
pub struct SettingsService {
    server_settings: Arc<dyn ServerSettingsRepository + Send + Sync>,
}

impl SettingsService {
    /// Creates a new settings service.
    pub fn new(server_settings: Arc<dyn ServerSettingsRepository + Send + Sync>) -> Self {
        Self { server_settings }
    }

    /// Retrieves server settings for a guild.
    /// Returns default settings if none exist.
    ///
    /// # Performance
    /// * DB calls: 1
    pub async fn get_server_settings(&self, guild_id: u64) -> Result<ServerSettings, ServiceError> {
        let result: Option<ServerSettingsEntity> = self.server_settings.select(&guild_id).await?;
        match result {
            Some(model) => Ok(model.settings.0),
            None => Ok(ServerSettings::default()),
        }
    }

    /// Updates server settings for a guild.
    ///
    /// # Performance
    /// * DB calls: 1
    pub async fn update_server_settings(
        &self,
        guild_id: u64,
        settings: ServerSettings,
    ) -> Result<(), ServiceError> {
        let model = ServerSettingsEntity {
            guild_id: guild_id.into(),
            settings: Json(settings),
        };
        self.server_settings.replace(&model).await?;
        Ok(())
    }
}
