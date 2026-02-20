//! Server settings service for centralized settings management.

use std::sync::Arc;

use crate::model::ServerSettings;
use crate::model::ServerSettingsModel;
use crate::repository::Repository;
use crate::repository::table::Table;
use crate::service::error::ServiceError;

/// Service for managing server settings.
/// Provides a single source of truth for all server configuration.
pub struct SettingsService {
    db: Arc<Repository>,
}

impl SettingsService {
    /// Creates a new settings service.
    pub fn new(db: Arc<Repository>) -> Self {
        Self { db }
    }

    /// Retrieves server settings for a guild.
    /// Returns default settings if none exist.
    ///
    /// # Performance
    /// * DB calls: 1
    pub async fn get_server_settings(&self, guild_id: u64) -> Result<ServerSettings, ServiceError> {
        match self.db.server_settings.select(&guild_id).await? {
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
        let model = ServerSettingsModel {
            guild_id,
            settings: sqlx::types::Json(settings),
        };
        self.db.server_settings.replace(&model).await?;
        Ok(())
    }
}
