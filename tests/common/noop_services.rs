use async_trait::async_trait;
use pwr_bot::entity::BotMetaKey;
use pwr_bot::entity::ServerSettings;
use pwr_bot::repo::error::DatabaseError;
use pwr_bot::service::error::ServiceError;
use pwr_bot::service::traits::InternalOps;
use pwr_bot::service::traits::SettingsProvider;

pub struct NoopSettingsProvider;

#[async_trait]
impl SettingsProvider for NoopSettingsProvider {
    async fn get_server_settings(&self, _guild_id: u64) -> Result<ServerSettings, ServiceError> {
        Ok(ServerSettings::default())
    }

    async fn update_server_settings(
        &self,
        _guild_id: u64,
        _settings: ServerSettings,
    ) -> Result<(), ServiceError> {
        Ok(())
    }
}

pub struct NoopInternalOps;

#[async_trait]
impl InternalOps for NoopInternalOps {
    async fn get_meta(&self, _key: BotMetaKey) -> Result<Option<String>, DatabaseError> {
        Ok(None)
    }

    async fn set_meta(&self, _key: BotMetaKey, _value: String) -> Result<(), DatabaseError> {
        Ok(())
    }
}
