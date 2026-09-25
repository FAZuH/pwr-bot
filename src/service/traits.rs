//! Business logic interfaces for host services.

use async_trait::async_trait;
use mockall::automock;

use crate::entity::*;
use crate::repo::error::DatabaseError;
use crate::service::error::ServiceError;
use crate::service::internal::DatabaseDump;

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
