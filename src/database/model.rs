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
/// Represents the base metadata for any subscribable content (manga, anime,
/// social media). The actual version history is tracked separately in
/// `FeedVersionModel`.
#[derive(FromRow, Serialize, Default, Clone, Debug)]
pub struct FeedModel {
    #[serde(default)]
    pub id: i32,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub description: String,
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
#[derive(FromRow, Serialize, Default, Clone)]
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
    pub channel_id: Option<String>,
    #[serde(default)]
    pub subscribe_role_id: Option<String>,
    #[serde(default)]
    pub unsubscribe_role_id: Option<String>,
}
