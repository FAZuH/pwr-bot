use std::borrow::Borrow;
use std::collections::HashMap;
use std::io::Write;
use std::ops::Deref;

use byteorder::ReadBytesExt;
use byteorder::WriteBytesExt;
use diesel::deserialize::FromSql;
use diesel::deserialize::FromSqlRow;
use diesel::expression::AsExpression;
use diesel::prelude::*;
use diesel::serialize::IsNull;
use diesel::serialize::ToSql;
use diesel::sql_types::*;
use serde::Deserialize;
use serde::Serialize;

use crate::repo::schema::bot_meta;
use crate::repo::schema::server_settings;

/// Newtype for `u64` values stored as `BIGINT`/`Int8`.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    AsExpression,
    FromSqlRow,
    Default,
    Serialize,
    Deserialize,
)]
#[diesel(sql_type = BigInt)]
pub struct DbU64(pub u64);

impl Deref for DbU64 {
    type Target = u64;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<u64> for DbU64 {
    fn from(v: u64) -> Self {
        Self(v)
    }
}

impl From<DbU64> for u64 {
    fn from(v: DbU64) -> Self {
        v.0
    }
}

impl Borrow<u64> for DbU64 {
    fn borrow(&self) -> &u64 {
        &self.0
    }
}

impl FromSql<BigInt, diesel::pg::Pg> for DbU64 {
    fn from_sql(value: diesel::pg::PgValue<'_>) -> diesel::deserialize::Result<Self> {
        let mut bytes = value.as_bytes();
        let val = bytes
            .read_i64::<byteorder::NetworkEndian>()
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
        Ok(DbU64(val as u64))
    }
}

impl ToSql<BigInt, diesel::pg::Pg> for DbU64 {
    fn to_sql<'b>(
        &'b self,
        out: &mut diesel::serialize::Output<'b, '_, diesel::pg::Pg>,
    ) -> diesel::serialize::Result {
        out.write_i64::<byteorder::NetworkEndian>(self.0 as i64)
            .map(|_| IsNull::No)
            .map_err(Into::into)
    }
}

/// Newtype for JSON values stored as `JSONB` in PostgreSQL.
#[derive(Debug, Clone, AsExpression, FromSqlRow, Serialize, Deserialize, Default)]
#[diesel(sql_type = Jsonb)]
pub struct Json<T>(pub T);

impl<T: Serialize + std::fmt::Debug> ToSql<Jsonb, diesel::pg::Pg> for Json<T> {
    fn to_sql<'b>(
        &'b self,
        out: &mut diesel::serialize::Output<'b, '_, diesel::pg::Pg>,
    ) -> diesel::serialize::Result {
        out.write_all(&[1])?;
        serde_json::to_writer(out, &self.0)
            .map(|_| IsNull::No)
            .map_err(Into::into)
    }
}

impl<T: for<'de> Deserialize<'de>> FromSql<Jsonb, diesel::pg::Pg> for Json<T> {
    fn from_sql(value: diesel::pg::PgValue<'_>) -> diesel::deserialize::Result<Self> {
        let bytes = value.as_bytes();
        if bytes.is_empty() || bytes[0] != 1 {
            return Err("Unsupported JSONB encoding version".into());
        }
        Ok(Json(serde_json::from_slice(&bytes[1..])?))
    }
}

// =============================================================================
// Table models
// =============================================================================

#[derive(Queryable, Selectable, Insertable, Identifiable, AsChangeset)]
#[diesel(table_name = server_settings)]
#[diesel(primary_key(guild_id))]
#[diesel(check_for_backend(diesel::pg::Pg))]
#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct ServerSettingsEntity {
    pub guild_id: DbU64,
    pub settings: Json<ServerSettings>,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct ServerSettings {
    /// All plugin settings stored as a flat map of plugin_id → settings object.
    /// Each entry is expected to have an `"enabled"` boolean field.
    /// Uses `#[serde(flatten)]` so existing JSONB `{"feeds": {...}, "voice": {...}}`
    /// deserializes directly without migration.
    #[serde(flatten)]
    pub plugin_settings: HashMap<String, serde_json::Value>,
}

impl ServerSettings {
    pub fn is_enabled(&self, plugin_id: &str) -> bool {
        self.plugin_settings
            .get(plugin_id)
            .and_then(|v| match v {
                serde_json::Value::Bool(b) => Some(*b),
                serde_json::Value::Object(obj) => obj.get("enabled").and_then(|v| v.as_bool()),
                _ => None,
            })
            .unwrap_or(false)
    }

    pub fn set_enabled(&mut self, plugin_id: &str, enabled: bool) {
        let entry = self
            .plugin_settings
            .entry(plugin_id.to_string())
            .or_insert_with(|| serde_json::json!({}));
        if let Some(obj) = entry.as_object_mut() {
            obj.insert("enabled".to_string(), serde_json::Value::Bool(enabled));
        }
    }
}

/// Key-value store for bot metadata.
#[derive(Queryable, Selectable, Insertable, Identifiable, AsChangeset)]
#[diesel(table_name = bot_meta)]
#[diesel(primary_key(key))]
#[diesel(check_for_backend(diesel::pg::Pg))]
#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct BotMetaEntity {
    pub key: String,
    pub value: String,
}

pub enum BotMetaKey {
    BotVersion,
}

impl From<&BotMetaKey> for String {
    fn from(value: &BotMetaKey) -> Self {
        match value {
            BotMetaKey::BotVersion => "bot_version".to_string(),
        }
    }
}

impl From<BotMetaKey> for String {
    fn from(value: BotMetaKey) -> Self {
        String::from(&value)
    }
}
