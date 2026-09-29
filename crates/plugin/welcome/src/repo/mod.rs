//! The welcome plugin's settings repository: the `welcome_settings` table it
//! owns, plus the copy-once legacy read that seeds it from
//! `server_settings.settings.welcome`.

pub mod error;
pub mod schema;

use diesel::prelude::*;
use diesel::sql_types::BigInt;
use diesel::sql_types::Jsonb;
use diesel_async::RunQueryDsl;
use pwr_plugin_protocol::ServerSettings;
use pwr_plugin_protocol::WelcomeSettings;
use serde_json::Value;

use crate::repo::error::DatabaseError;
use crate::repo::schema::welcome_settings;
use crate::storage::DbPool;
use crate::storage::Store;

/// One guild's welcome settings row.
#[derive(Queryable, Selectable, Insertable, AsChangeset)]
#[diesel(table_name = welcome_settings)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct WelcomeSettingsEntity {
    pub guild_id: i64,
    pub enabled: bool,
    pub channel_id: Option<String>,
    pub primary_color: Option<String>,
    pub template_id: Option<String>,
    pub messages: Option<Value>,
}

#[derive(QueryableByName)]
#[diesel(check_for_backend(diesel::pg::Pg))]
struct LegacyServerSettingsRow {
    #[diesel(sql_type = Jsonb)]
    settings: Value,
}

/// The plugin's own storage: a pool over its database plus the embedded
/// migrations that create its tables.
#[derive(Clone)]
pub struct Repository {
    pool: DbPool,
    store: Store,
}

impl Repository {
    pub async fn connect(db_url: impl Into<String>) -> anyhow::Result<Self> {
        let store = Store::connect(db_url).await?;
        let pool = store.pool().clone();
        Ok(Self { pool, store })
    }

    pub async fn migrate(&self) -> anyhow::Result<Vec<String>> {
        self.store.migrate().await
    }

    pub fn pool(&self) -> &DbPool {
        self.store.pool()
    }

    /// The guild's welcome settings: the plugin's own row when one exists,
    /// otherwise the legacy `server_settings.settings.welcome` section copied
    /// into an own row. The copy happens once per guild — every later read
    /// answers from `welcome_settings` alone.
    pub async fn get_settings(&self, guild_id: u64) -> Result<WelcomeSettings, DatabaseError> {
        if let Some(settings) = self.select_settings(guild_id).await? {
            return Ok(settings);
        }
        let settings = self.legacy_settings(guild_id).await?;
        self.replace_settings(&WelcomeSettingsEntity::from_settings(
            guild_id,
            settings.clone(),
        ))
        .await?;
        Ok(settings)
    }

    /// Persists the guild's welcome settings wholesale.
    pub async fn update_settings(
        &self,
        guild_id: u64,
        settings: &WelcomeSettings,
    ) -> Result<(), DatabaseError> {
        self.replace_settings(&WelcomeSettingsEntity::from_settings(
            guild_id,
            settings.clone(),
        ))
        .await
    }

    async fn select_settings(
        &self,
        guild_id: u64,
    ) -> Result<Option<WelcomeSettings>, DatabaseError> {
        let mut connection = self.pool.get().await?;
        let row = welcome_settings::table
            .find(guild_id as i64)
            .select(WelcomeSettingsEntity::as_select())
            .first(&mut connection)
            .await
            .optional()?;
        row.map(WelcomeSettingsEntity::into_settings).transpose()
    }

    async fn replace_settings(
        &self,
        settings: &WelcomeSettingsEntity,
    ) -> Result<(), DatabaseError> {
        let mut connection = self.pool.get().await?;
        diesel::insert_into(welcome_settings::table)
            .values(settings)
            .on_conflict(welcome_settings::guild_id)
            .do_update()
            .set((
                welcome_settings::enabled.eq(settings.enabled),
                welcome_settings::channel_id.eq(&settings.channel_id),
                welcome_settings::primary_color.eq(&settings.primary_color),
                welcome_settings::template_id.eq(&settings.template_id),
                welcome_settings::messages.eq(&settings.messages),
            ))
            .execute(&mut connection)
            .await?;
        Ok(())
    }

    /// The legacy `server_settings.settings.welcome` section for the guild;
    /// defaults when the guild has no legacy row or a malformed snapshot.
    async fn legacy_settings(&self, guild_id: u64) -> Result<WelcomeSettings, DatabaseError> {
        let mut connection = self.pool.get().await?;
        let row = diesel::sql_query("SELECT settings FROM server_settings WHERE guild_id = $1")
            .bind::<BigInt, _>(guild_id as i64)
            .load::<LegacyServerSettingsRow>(&mut connection)
            .await?
            .into_iter()
            .next();
        Ok(row
            .and_then(|row| serde_json::from_value::<ServerSettings>(row.settings).ok())
            .map(|settings| settings.welcome)
            .unwrap_or_default())
    }
}

impl WelcomeSettingsEntity {
    fn into_settings(self) -> Result<WelcomeSettings, DatabaseError> {
        Ok(WelcomeSettings {
            enabled: Some(self.enabled),
            channel_id: self.channel_id,
            primary_color: self.primary_color,
            template_id: self.template_id,
            messages: self
                .messages
                .map(serde_json::from_value)
                .transpose()
                .map_err(|error| DatabaseError::ParseError {
                    message: format!("`welcome_settings.messages` is not a string list: {error}"),
                })?,
        })
    }

    fn from_settings(guild_id: u64, settings: WelcomeSettings) -> Self {
        Self {
            guild_id: guild_id as i64,
            enabled: settings.enabled.unwrap_or(false),
            channel_id: settings.channel_id,
            primary_color: settings.primary_color,
            template_id: settings.template_id,
            messages: settings
                .messages
                .map(|messages| Value::Array(messages.into_iter().map(Value::String).collect())),
        }
    }
}
