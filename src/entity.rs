use std::borrow::Borrow;
use std::hash::Hash;
use std::ops::Deref;

use chrono::DateTime;
use chrono::NaiveDateTime;
use chrono::Utc;
use diesel::backend::Backend;
use diesel::deserialize::FromSql;
use diesel::deserialize::FromSqlRow;
use diesel::expression::AsExpression;
use diesel::prelude::*;
use diesel::serialize::ToSql;
use diesel::sql_types::*;
use diesel::sqlite::Sqlite;
use serde::Deserialize;
use serde::Serialize;

use crate::repo::schema::bot_meta;
use crate::repo::schema::feed_items;
use crate::repo::schema::feed_subscriptions;
use crate::repo::schema::feeds;
use crate::repo::schema::server_settings;
use crate::repo::schema::subscribers;
use crate::repo::schema::voice_sessions;

// =============================================================================
// Custom type wrappers
// =============================================================================

/// Newtype for `u64` values stored as `BIGINT` (i64) in SQLite.
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

impl<DB> FromSql<BigInt, DB> for DbU64
where
    DB: Backend,
    i64: FromSql<BigInt, DB>,
{
    fn from_sql(bytes: DB::RawValue<'_>) -> diesel::deserialize::Result<Self> {
        Ok(DbU64(i64::from_sql(bytes)? as u64))
    }
}

impl ToSql<BigInt, Sqlite> for DbU64 {
    fn to_sql<'b>(
        &'b self,
        out: &mut diesel::serialize::Output<'b, '_, Sqlite>,
    ) -> diesel::serialize::Result {
        out.set_value(self.0 as i64);
        Ok(diesel::serialize::IsNull::No)
    }
}

/// Newtype for JSON values stored as `TEXT` in SQLite.
#[derive(Debug, Clone, AsExpression, FromSqlRow, Serialize, Deserialize, Default)]
#[diesel(sql_type = Text)]
pub struct Json<T>(pub T);

impl<T: Serialize + std::fmt::Debug> ToSql<Text, Sqlite> for Json<T> {
    fn to_sql<'b>(
        &'b self,
        out: &mut diesel::serialize::Output<'b, '_, Sqlite>,
    ) -> diesel::serialize::Result {
        out.set_value(serde_json::to_string(&self.0)?);
        Ok(diesel::serialize::IsNull::No)
    }
}

impl<T: for<'de> Deserialize<'de>> FromSql<Text, Sqlite> for Json<T> {
    fn from_sql(bytes: <Sqlite as Backend>::RawValue<'_>) -> diesel::deserialize::Result<Self> {
        let s = <String as FromSql<Text, Sqlite>>::from_sql(bytes)?;
        Ok(Json(serde_json::from_str(&s)?))
    }
}

// =============================================================================
// Enums
// =============================================================================

/// Notification target type for feed updates.
#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq, AsExpression, FromSqlRow,
)]
#[diesel(sql_type = Text)]
#[serde(rename_all = "lowercase")]
pub enum SubscriberType {
    #[default]
    Guild,
    Dm,
}

impl ToSql<Text, Sqlite> for SubscriberType {
    fn to_sql<'b>(
        &'b self,
        out: &mut diesel::serialize::Output<'b, '_, Sqlite>,
    ) -> diesel::serialize::Result {
        match self {
            SubscriberType::Guild => <str as ToSql<Text, Sqlite>>::to_sql("guild", out),
            SubscriberType::Dm => <str as ToSql<Text, Sqlite>>::to_sql("dm", out),
        }
    }
}

impl FromSql<Text, Sqlite> for SubscriberType {
    fn from_sql(bytes: <Sqlite as Backend>::RawValue<'_>) -> diesel::deserialize::Result<Self> {
        match <String as FromSql<Text, Sqlite>>::from_sql(bytes)?.as_str() {
            "guild" => Ok(SubscriberType::Guild),
            "dm" => Ok(SubscriberType::Dm),
            other => Err(format!("Unknown subscriber type: {other}").into()),
        }
    }
}

// =============================================================================
// Table models
// =============================================================================

/// A content source that can be monitored for updates.
#[derive(Queryable, Selectable, Insertable, Identifiable, AsChangeset)]
#[diesel(table_name = feeds)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct FeedEntity {
    pub id: i32,
    pub name: String,
    pub description: String,
    pub platform_id: String,
    pub source_id: String,
    pub items_id: String,
    pub source_url: String,
    pub cover_url: String,
    pub tags: String,
}

/// A specific version or episode of a feed.
#[derive(Queryable, Selectable, Insertable, Identifiable, AsChangeset)]
#[diesel(table_name = feed_items)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct FeedItemEntity {
    pub id: i32,
    pub feed_id: i32,
    pub description: String,
    pub published: NaiveDateTime,
}

/// A notification target that can receive feed updates.
#[derive(Queryable, Selectable, Insertable, Identifiable, AsChangeset)]
#[diesel(table_name = subscribers)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct SubscriberEntity {
    pub id: i32,
    #[diesel(column_name = type_)]
    pub r#type: SubscriberType,
    pub target_id: String,
}

/// Links subscribers to the feeds they're monitoring.
#[derive(Queryable, Selectable, Insertable, Identifiable, AsChangeset)]
#[diesel(table_name = feed_subscriptions)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct FeedSubscriptionEntity {
    pub id: i32,
    pub feed_id: i32,
    pub subscriber_id: i32,
}

#[derive(Queryable, Selectable, Insertable, Identifiable, AsChangeset)]
#[diesel(table_name = server_settings)]
#[diesel(primary_key(guild_id))]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct ServerSettingsEntity {
    pub guild_id: DbU64,
    pub settings: Json<ServerSettings>,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct ServerSettings {
    #[serde(default)]
    pub feeds: FeedsSettings,
    #[serde(default)]
    pub voice: VoiceSettings,
    #[serde(default)]
    pub welcome: WelcomeSettings,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq, Eq)]
pub struct WelcomeSettings {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub channel_id: Option<String>,
    #[serde(default)]
    pub primary_color: Option<String>,
    #[serde(default)]
    pub template_id: Option<String>,
    #[serde(default)]
    pub messages: Option<Vec<String>>,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct FeedsSettings {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub channel_id: Option<String>,
    #[serde(default)]
    pub subscribe_role_id: Option<String>,
    #[serde(default)]
    pub unsubscribe_role_id: Option<String>,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct VoiceSettings {
    pub enabled: Option<bool>,
}

/// Diesel-compatible struct for voice_sessions queries.
#[derive(Queryable, Selectable)]
#[diesel(table_name = voice_sessions)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct DbVoiceSession {
    pub id: i32,
    pub user_id: DbU64,
    pub guild_id: DbU64,
    pub channel_id: DbU64,
    pub join_time: NaiveDateTime,
    pub leave_time: NaiveDateTime,
    pub is_active: bool,
}

/// Diesel-compatible struct for inserting/updating voice sessions.
#[derive(Insertable, AsChangeset)]
#[diesel(table_name = voice_sessions)]
pub struct NewDbVoiceSession {
    pub user_id: DbU64,
    pub guild_id: DbU64,
    pub channel_id: DbU64,
    pub join_time: NaiveDateTime,
    pub leave_time: NaiveDateTime,
    pub is_active: bool,
}

/// Domain entity for voice channel sessions.
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
            join_time: self.join_time.naive_utc(),
            leave_time: self.leave_time.naive_utc(),
            is_active: self.is_active,
        }
    }
}

impl From<DbVoiceSession> for VoiceSessionsEntity {
    fn from(db: DbVoiceSession) -> Self {
        Self {
            id: db.id,
            user_id: db.user_id.into(),
            guild_id: db.guild_id.into(),
            channel_id: db.channel_id.into(),
            join_time: db.join_time.and_utc(),
            leave_time: db.leave_time.and_utc(),
            is_active: db.is_active,
        }
    }
}

#[derive(Serialize, Default, Clone, Debug, PartialEq, Eq)]
pub struct VoiceLeaderboardEntry {
    pub user_id: u64,
    pub total_duration: i64,
}

#[derive(QueryableByName)]
#[diesel(table_name = voice_sessions)]
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

#[derive(QueryableByName)]
pub struct FeedWithLatestItemRow {
    #[diesel(sql_type = Integer)]
    pub id: i32,
    #[diesel(sql_type = Text)]
    pub name: String,
    #[diesel(sql_type = Text)]
    pub description: String,
    #[diesel(sql_type = Text)]
    pub platform_id: String,
    #[diesel(sql_type = Text)]
    pub source_id: String,
    #[diesel(sql_type = Text)]
    pub items_id: String,
    #[diesel(sql_type = Text)]
    pub source_url: String,
    #[diesel(sql_type = Text)]
    pub cover_url: String,
    #[diesel(sql_type = Text)]
    pub tags: String,

    #[diesel(sql_type = Nullable<Integer>)]
    pub item_id: Option<i32>,
    #[diesel(sql_type = Nullable<Text>)]
    pub item_description: Option<String>,
    #[diesel(sql_type = Nullable<Timestamp>)]
    pub item_published: Option<NaiveDateTime>,
}

use derive_builder::Builder;

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

/// Daily voice activity aggregation for a specific user.
#[derive(QueryableByName, Serialize, Deserialize, Default, Clone, Debug)]
pub struct VoiceDailyActivity {
    #[diesel(sql_type = diesel::sql_types::Date)]
    pub day: chrono::NaiveDate,
    #[diesel(sql_type = BigInt)]
    pub total_seconds: i64,
}

/// Guild daily statistics aggregation.
#[derive(QueryableByName, Serialize, Deserialize, Default, Clone, Debug)]
pub struct GuildDailyStats {
    #[diesel(sql_type = diesel::sql_types::Date)]
    pub day: chrono::NaiveDate,
    #[diesel(sql_type = BigInt)]
    pub value: i64,
}

/// Key-value store for bot metadata.
#[derive(Queryable, Selectable, Insertable, Identifiable, AsChangeset)]
#[diesel(table_name = bot_meta)]
#[diesel(primary_key(key))]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct BotMetaEntity {
    pub key: String,
    pub value: String,
}

pub enum BotMetaKey {
    VoiceHeartbeat,
    BotVersion,
}

impl From<&BotMetaKey> for String {
    fn from(value: &BotMetaKey) -> Self {
        match value {
            BotMetaKey::VoiceHeartbeat => "voice_heartbeat".to_string(),
            BotMetaKey::BotVersion => "bot_version".to_string(),
        }
    }
}

impl From<BotMetaKey> for String {
    fn from(value: BotMetaKey) -> Self {
        String::from(&value)
    }
}
