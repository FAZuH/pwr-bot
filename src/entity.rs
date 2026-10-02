use std::borrow::Borrow;
use std::hash::Hash;
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
use crate::repo::schema::guild_plugins;

// =============================================================================
// Custom type wrappers
// =============================================================================

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

// =============================================================================
// Table models
// =============================================================================

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

/// The per-guild enable/disable state of a plugin.
///
/// Wired to command registration in #113; this layer only persists the flag.
#[derive(Queryable, Selectable, Insertable, Identifiable)]
#[diesel(table_name = guild_plugins)]
#[diesel(primary_key(guild_id, plugin_name))]
#[diesel(check_for_backend(diesel::pg::Pg))]
#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct GuildPluginEntity {
    pub guild_id: DbU64,
    pub plugin_name: String,
    pub enabled: bool,
}
