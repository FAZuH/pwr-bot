use async_trait::async_trait;

use crate::entity::BotMetaKey;
use crate::entity::ServerSettings;
use crate::repo::error::DatabaseError;
use crate::service::error::ServiceError;

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
}
