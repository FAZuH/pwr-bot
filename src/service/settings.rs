//! Server settings service for centralized settings management.

use std::sync::Arc;

use crate::entity::Json;
use crate::entity::ServerSettings;
use crate::entity::ServerSettingsEntity;
use crate::repo::traits::*;
use crate::service::error::ServiceError;
use crate::service::traits::SettingsProvider;

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
        let result: Option<ServerSettingsEntity> =
            self.server_settings.select(&guild_id).await?;
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
