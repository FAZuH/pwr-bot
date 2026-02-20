use chrono::DateTime;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;
use sqlx::FromRow;

/// Notification target type for feed updates.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, sqlx::Type, Default, PartialEq, Eq)]
#[sqlx(type_name = "text", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum SubscriberType {
    #[default]
    Guild,
    Dm,
}

/// A content source that can be monitored for updates.
///
/// Represents metadata for a subscribable content source (manga series, anime)
/// on a specific platform. The actual version history is tracked separately in
/// [`FeedItemModel`].
///
/// # Hierarchy
/// - **Platform**: External service (AniList, MangaDex, Comick)
/// - **Feed/Source**: Specific content on that platform (One Punch Man on MangaDex)
/// - **Feed Items**: Individual updates (chapters, episodes)
#[derive(FromRow, Serialize, Default, Clone, Debug)]
pub struct FeedModel {
    #[serde(default)]
    pub id: i32,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Platform identifier (e.g., "mangadex", "anilist", "comick")
    #[serde(default)]
    pub platform_id: String,
    /// Platform-specific identifier for this feed source
    #[serde(default)]
    pub source_id: String,
    /// Platform-specific identifier for fetching feed items
    #[serde(default)]
    pub items_id: String,
    /// Feed source URL
    #[serde(default)]
    pub source_url: String,
    /// Cover image URL (manga covers, anime posters)
    #[serde(default)]
    pub cover_url: String,
    /// Comma-separated tags for categorization (e.g., "manga,ongoing")
    #[serde(default)]
    pub tags: String,
}

/// A specific version or episode of a feed.
///
/// Tracks the history of updates for a content source. Each new episode,
/// chapter, or post creates a new version entry. The latest version can be
/// determined by querying for the most recent `published` timestamp.
#[derive(FromRow, Serialize, Default, Clone, Debug)]
pub struct FeedItemModel {
    #[serde(default)]
    pub id: i32,
    #[serde(default)]
    pub feed_id: i32,
    /// Human-readable version identifier (e.g., "S2E1", "Chapter 127")
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub published: DateTime<Utc>,
}

/// A notification target that can receive feed updates.
///
/// Represents either a Discord guild channel or a direct message conversation
/// with a user. Multiple subscribers can follow the same feed, and a single
/// subscriber can follow multiple feeds (via `FeedSubscriptionModel`).
#[derive(FromRow, Serialize, Default, Clone)]
pub struct SubscriberModel {
    #[serde(default)]
    pub id: i32,
    #[serde(default)]
    pub r#type: SubscriberType,
    /// Discord snowflake ID (channel ID for Guild, user ID for DM)
    #[serde(default)]
    pub target_id: String,
}

/// Links subscribers to the feeds they're monitoring.
///
/// Junction table implementing the many-to-many relationship between feeds
/// and subscribers. When a new `FeedVersionModel` is published, query this
/// table to find all subscribers that need to be notified.
#[derive(FromRow, Serialize, Default, Clone)]
pub struct FeedSubscriptionModel {
    #[serde(default)]
    pub id: i32,
    #[serde(default)]
    pub feed_id: i32,
    #[serde(default)]
    pub subscriber_id: i32,
}

#[derive(FromRow, Serialize, Deserialize, Default, Clone, Debug)]
pub struct ServerSettingsModel {
    #[serde(default)]
    #[sqlx(try_from = "i64")]
    pub guild_id: u64,
    pub settings: sqlx::types::Json<ServerSettings>,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct ServerSettings {
    #[serde(default)]
    pub feeds: FeedsSettings,
    #[serde(default)]
    pub voice: VoiceSettings,
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

#[derive(FromRow, Serialize, Default, Clone, Debug)]
pub struct VoiceLeaderboardEntry {
    #[sqlx(try_from = "i64")]
    pub user_id: u64,
    pub total_duration: i64, // Duration in seconds
}

#[derive(FromRow)]
pub struct FeedWithLatestItemRow {
    // Feed fields
    pub id: i32,
    pub name: String,
    pub description: String,
    pub platform_id: String,
    pub source_id: String,
    pub items_id: String,
    pub source_url: String,
    pub cover_url: String,
    pub tags: String,

    // FeedItem fields (nullable because of LEFT JOIN)
    pub item_id: Option<i32>,
    pub item_description: Option<String>,
    pub item_published: Option<DateTime<Utc>>,
}

#[derive(FromRow, Serialize, Default, Clone)]
pub struct VoiceSessionsModel {
    #[serde(default)]
    pub id: i32,
    #[serde(default)]
    #[sqlx(try_from = "i64")]
    pub user_id: u64,
    #[serde(default)]
    #[sqlx(try_from = "i64")]
    pub guild_id: u64,
    #[serde(default)]
    #[sqlx(try_from = "i64")]
    pub channel_id: u64,
    #[serde(default)]
    pub join_time: DateTime<Utc>,
    #[serde(default)]
    pub leave_time: DateTime<Utc>,
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
#[derive(FromRow, Serialize, Default, Clone, Debug)]
pub struct VoiceDailyActivity {
    /// The date (day) of the activity
    pub day: chrono::NaiveDate,
    /// Total duration in seconds for that day
    pub total_seconds: i64,
}

/// Guild daily statistics aggregation.
#[derive(FromRow, Serialize, Default, Clone, Debug)]
pub struct GuildDailyStats {
    /// The date (day) of the activity
    pub day: chrono::NaiveDate,
    /// For average time: average seconds per active user
    /// For user count: number of unique active users
    pub value: i64,
}

/// Key-value store for bot metadata (version, heartbeat timestamp, etc.)
#[derive(FromRow, Serialize, Deserialize, Default, Clone, Debug)]
pub struct BotMetaModel {
    #[serde(default)]
    pub key: String,
    #[serde(default)]
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

impl ToString for BotMetaKey {
    fn to_string(&self) -> String {
        String::from(self)
    }
}
