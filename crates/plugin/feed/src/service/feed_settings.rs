use diesel::prelude::*;
use diesel::sql_types::BigInt;
use diesel::sql_types::Jsonb;
use diesel_async::RunQueryDsl;
use pwr_plugin_protocol::FeedsSettings;
use pwr_plugin_protocol::ServerSettings;
use serde_json::Value;

use crate::repo::Repository;
use crate::repo::error::DatabaseError;
use crate::service::error::ServiceError;

#[derive(Queryable, Selectable, Insertable, AsChangeset)]
#[diesel(table_name = crate::repo::schema::feed_settings)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct FeedSettingsEntity {
    pub guild_id: i64,
    pub enabled: bool,
    pub channel_id: Option<String>,
    pub subscribe_role_id: Option<String>,
    pub unsubscribe_role_id: Option<String>,
}

#[derive(QueryableByName)]
#[diesel(check_for_backend(diesel::pg::Pg))]
struct LegacyServerSettingsRow {
    #[diesel(sql_type = Jsonb)]
    settings: Value,
}

pub struct FeedSettingsService {
    repository: Repository,
}

impl FeedSettingsService {
    pub fn new(repository: Repository) -> Self {
        Self { repository }
    }

    pub async fn get(&self, guild_id: u64) -> Result<FeedsSettings, ServiceError> {
        if let Some(settings) = self.repository.feed_settings.select(guild_id).await? {
            return Ok(settings.into());
        }

        let settings = self.legacy_settings(guild_id).await?;
        self.repository
            .feed_settings
            .replace(&FeedSettingsEntity::from_settings(
                guild_id,
                settings.clone(),
            ))
            .await?;
        Ok(settings)
    }

    pub async fn update(&self, guild_id: u64, settings: FeedsSettings) -> Result<(), ServiceError> {
        self.repository
            .feed_settings
            .replace(&FeedSettingsEntity::from_settings(guild_id, settings))
            .await?;
        Ok(())
    }

    async fn legacy_settings(&self, guild_id: u64) -> Result<FeedsSettings, DatabaseError> {
        let mut connection = self.repository.pool().get().await?;
        let row = diesel::sql_query("SELECT settings FROM server_settings WHERE guild_id = $1")
            .bind::<BigInt, _>(guild_id as i64)
            .load::<LegacyServerSettingsRow>(&mut connection)
            .await?
            .into_iter()
            .next();
        Ok(row
            .and_then(|row| serde_json::from_value::<ServerSettings>(row.settings).ok())
            .map(|settings| settings.feeds)
            .unwrap_or_default())
    }
}

impl FeedSettingsEntity {
    fn into_settings(self) -> FeedsSettings {
        FeedsSettings {
            enabled: Some(self.enabled),
            channel_id: self.channel_id,
            subscribe_role_id: self.subscribe_role_id,
            unsubscribe_role_id: self.unsubscribe_role_id,
        }
    }
}

impl From<FeedSettingsEntity> for FeedsSettings {
    fn from(value: FeedSettingsEntity) -> Self {
        value.into_settings()
    }
}

impl FeedSettingsEntity {
    fn from_settings(guild_id: u64, settings: FeedsSettings) -> Self {
        Self {
            guild_id: guild_id as i64,
            enabled: settings.enabled.unwrap_or(true),
            channel_id: settings.channel_id,
            subscribe_role_id: settings.subscribe_role_id,
            unsubscribe_role_id: settings.unsubscribe_role_id,
        }
    }
}
