#![allow(clippy::redundant_field_names)]

use std::borrow::Borrow;
use std::hash::Hash;
use std::ops::Deref;

use byteorder::ReadBytesExt;
use byteorder::WriteBytesExt;
use chrono::DateTime;
use chrono::SubsecRound;
use chrono::Utc;
use derive_builder::Builder;
use diesel::deserialize::FromSql;
use diesel::deserialize::FromSqlRow;
use diesel::expression::AsExpression;
use diesel::prelude::*;
use diesel::serialize::IsNull;
use diesel::serialize::ToSql;
use diesel::sql_types::BigInt;
use serde::Deserialize;
use serde::Serialize;

use crate::repo::schema::voice_sessions;
use crate::repo::schema::voice_settings;

/// Newtype for `u64` values stored as PostgreSQL `BIGINT`.
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
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl From<DbU64> for u64 {
    fn from(value: DbU64) -> Self {
        value.0
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
        let value = bytes
            .read_i64::<byteorder::NetworkEndian>()
            .map_err(|error| Box::new(error) as Box<dyn std::error::Error + Send + Sync>)?;
        Ok(Self(value as u64))
    }
}

impl ToSql<BigInt, diesel::pg::Pg> for DbU64 {
    fn to_sql<'b>(
        &'b self,
        output: &mut diesel::serialize::Output<'b, '_, diesel::pg::Pg>,
    ) -> diesel::serialize::Result {
        output
            .write_i64::<byteorder::NetworkEndian>(self.0 as i64)
            .map(|_| IsNull::No)
            .map_err(Into::into)
    }
}

/// Diesel-compatible voice session row.
#[derive(Queryable, Selectable)]
#[diesel(table_name = voice_sessions)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct DbVoiceSession {
    pub id: i32,
    pub user_id: DbU64,
    pub guild_id: DbU64,
    pub channel_id: DbU64,
    pub join_time: DateTime<Utc>,
    pub leave_time: DateTime<Utc>,
    pub is_active: bool,
}

/// Insertable and change-set representation of a voice session.
#[derive(Insertable, AsChangeset)]
#[diesel(table_name = voice_sessions)]
pub struct NewDbVoiceSession {
    pub user_id: DbU64,
    pub guild_id: DbU64,
    pub channel_id: DbU64,
    pub join_time: DateTime<Utc>,
    pub leave_time: DateTime<Utc>,
    pub is_active: bool,
}

/// A voice channel session.
#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq, Eq)]
pub struct VoiceSessionsEntity {
    pub id: i32,
    pub user_id: u64,
    pub guild_id: u64,
    pub channel_id: u64,
    pub join_time: DateTime<Utc>,
    pub leave_time: DateTime<Utc>,
    pub is_active: bool,
}

impl VoiceSessionsEntity {
    pub fn to_insertable(&self) -> NewDbVoiceSession {
        NewDbVoiceSession {
            user_id: self.user_id.into(),
            guild_id: self.guild_id.into(),
            channel_id: self.channel_id.into(),
            join_time: self.join_time.trunc_subsecs(6),
            leave_time: self.leave_time.trunc_subsecs(6),
            is_active: self.is_active,
        }
    }
}

impl From<DbVoiceSession> for VoiceSessionsEntity {
    fn from(row: DbVoiceSession) -> Self {
        Self {
            id: row.id,
            user_id: row.user_id.into(),
            guild_id: row.guild_id.into(),
            channel_id: row.channel_id.into(),
            join_time: row.join_time,
            leave_time: row.leave_time,
            is_active: row.is_active,
        }
    }
}

/// One ranked voice-activity entry.
#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq, Eq)]
pub struct VoiceLeaderboardEntry {
    pub user_id: u64,
    pub total_duration: i64,
}

/// SQL projection used by the leaderboard queries.
#[allow(clippy::redundant_field_names)]
#[derive(QueryableByName)]
pub struct VoiceLeaderboardRow {
    #[diesel(sql_type = BigInt)]
    pub user_id: DbU64,
    #[diesel(sql_type = BigInt)]
    pub total_duration: i64,
}

impl From<VoiceLeaderboardRow> for VoiceLeaderboardEntry {
    fn from(row: VoiceLeaderboardRow) -> Self {
        Self {
            user_id: row.user_id.into(),
            total_duration: row.total_duration,
        }
    }
}

/// Filters accepted by a leaderboard query.
#[derive(Builder, Clone)]
#[builder(pattern = "immutable")]
pub struct VoiceLeaderboardOpt {
    pub guild_id: u64,
    #[builder(default)]
    pub offset: Option<u32>,
    #[builder(default)]
    pub limit: Option<u32>,
    #[builder(default)]
    pub since: Option<DateTime<Utc>>,
    #[builder(default)]
    pub until: Option<DateTime<Utc>>,
}

/// Daily activity for one user.
#[allow(clippy::redundant_field_names)]
#[derive(QueryableByName, Serialize, Deserialize, Default, Clone, Debug, PartialEq, Eq)]
pub struct VoiceDailyActivity {
    #[diesel(sql_type = diesel::sql_types::Date)]
    pub day: chrono::NaiveDate,
    #[diesel(sql_type = BigInt)]
    pub total_seconds: i64,
}

/// Daily aggregate for a guild.
#[allow(clippy::redundant_field_names)]
#[derive(QueryableByName, Serialize, Deserialize, Default, Clone, Debug, PartialEq, Eq)]
pub struct GuildDailyStats {
    #[diesel(sql_type = diesel::sql_types::Date)]
    pub day: chrono::NaiveDate,
    #[diesel(sql_type = BigInt)]
    pub value: i64,
}

/// Plugin-owned voice setting row.
#[derive(Queryable, Selectable, Insertable, Identifiable, AsChangeset)]
#[diesel(table_name = voice_settings)]
#[diesel(primary_key(guild_id))]
#[diesel(check_for_backend(diesel::pg::Pg))]
#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq, Eq)]
pub struct VoiceSettingsEntity {
    pub guild_id: DbU64,
    pub enabled: bool,
}

/// The plugin's voice setting value.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct VoiceSettings {
    pub enabled: bool,
}

impl Default for VoiceSettings {
    fn default() -> Self {
        Self { enabled: true }
    }
}
