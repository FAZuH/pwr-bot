use std::fmt::Display;

use chrono::DateTime;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;
use sqlx::FromRow;

use crate::database::error::DatabaseError;

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

impl SubscriberModel {
    /// Parses the target_id for a Guild subscriber into (guild_id, channel_id).
    ///
    /// # Errors
    /// Returns [`DatabaseError::ParseError`] if called on a
    /// non-Guild subscriber or if target_id format is invalid.
    pub fn parse_guild_target_id(&self) -> Result<(&str, &str), DatabaseError> {
        if self.r#type != SubscriberType::Guild {
            return Err(DatabaseError::ParseError {
                message: format!(
                    "Cannot parse guild target_id from {:?} subscriber",
                    self.r#type
                ),
            });
        }

        self.target_id
            .split_once(":")
            .ok_or_else(|| DatabaseError::InternalError {
                message: format!("Invalid target_id format: {}", self.target_id),
            })
    }

    /// Formats a guild_id and channel_id into the target_id format used by Guild subscribers.
    pub fn format_guild_target_id(guild_id: impl Display, channel_id: impl Display) -> String {
        format!("{}:{}", guild_id, channel_id)
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_guild_target_id() {
        let gid = "123456";
        let cid = "789012";
        let target = SubscriberModel::format_guild_target_id(gid, cid);
        assert_eq!(target, "123456:789012");
    }

    #[test]
    fn test_parse_guild_target_id() {
        let sub = SubscriberModel {
            r#type: SubscriberType::Guild,
            target_id: "123:456".to_string(),
            ..Default::default()
        };

        let (gid, cid) = sub.parse_guild_target_id().expect("Should parse");
        assert_eq!(gid, "123");
        assert_eq!(cid, "456");
    }

    #[test]
    fn test_parse_guild_target_id_invalid_type() {
        let sub = SubscriberModel {
            r#type: SubscriberType::Dm,
            target_id: "123:456".to_string(),
            ..Default::default()
        };

        let res = sub.parse_guild_target_id();
        assert!(matches!(res, Err(DatabaseError::ParseError { .. })));
    }

    #[test]
    fn test_parse_guild_target_id_invalid_format() {
        let sub = SubscriberModel {
            r#type: SubscriberType::Guild,
            target_id: "123456".to_string(),
            ..Default::default()
        };

        let res = sub.parse_guild_target_id();
        assert!(matches!(res, Err(DatabaseError::InternalError { .. })));
    }
}
