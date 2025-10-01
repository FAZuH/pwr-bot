use chrono::{DateTime, Utc};

use super::Event;

/// Event fired when a new version/episode of a feed is published.
///
/// Contains both the previous and current version identifiers to enable
/// delta notifications (e.g., "Updated from Chapter 50 to Chapter 51").
#[derive(Clone, Debug)]
pub struct FeedUpdateEvent {
    pub feed_id: i32,
    pub version_id: i32,
    pub title: String,
    pub previous_version: String,
    pub current_version: String,
    pub url: String,
    pub published: DateTime<Utc>,
}

impl Event for FeedUpdateEvent {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
