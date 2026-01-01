//! Feed platform integrations and content monitoring.
//!
//! This module provides abstractions for integrating with external content platforms
//! (MangaDex, AniList, Comick) and fetching updates from them.
//!
//! # Terms
//!
//! - **Platform**: External service that hosts content (e.g., MangaDex, AniList)
//! - **Feed Source**: Specific content source on a platform (e.g., "One Punch Man" on MangaDex)
//! - **Feed Item**: Individual updates within a source (e.g., chapters, episodes)
//!
//! # Usage
//!
//! ```rust
//! use pwr_bot::feed::mangadex_platform::MangaDexPlatform;
//!
//! let platform = MangaDexPlatform::new();
//! let source = platform.fetch_source("id").await?;
//! let latest_item = platform.fetch_latest("id").await?;
//! ```
//!
//! # Implementing New Platforms
//!
//! To add support for a new platform:
//!
//! 1. Implement the [`Platform`] trait
//! 2. Define platform-specific error handling
//! 3. Add rate limiting if needed
//! 4. Register in [`crate::feed::platforms::Platforms`] collection
//!
//! See [`MangaDexPlatform`] for a reference implementation.

pub mod anilist_platform;
pub mod error;
pub mod mangadex_platform;
pub mod platforms;

use async_trait::async_trait;
use chrono::DateTime;
use chrono::Utc;

use crate::feed::error::FeedError;
use crate::feed::error::UrlParseError;

#[derive(Clone, Debug, Default)]
pub struct PlatformInfo {
    /// The name of the platform, e.g., "MangaDex", "AniList Anime"
    pub name: String,
    /// What do you call the item the feeds of this platform publishes? e.g., "Episode", "Chapter"
    pub feed_item_name: String,
    /// api.feed.tld
    pub api_hostname: String,
    /// feed.tld
    pub api_domain: String,
    /// https://api.feed.tld
    pub api_url: String,
    /// © feed 2067
    pub copyright_notice: String,
    /// https://anilist.co/img/icons/icon.svg
    pub logo_url: String,
    /// Platform tags. Mainly used for grouping and filtering
    pub tags: String,
}

#[derive(Clone, Debug)]
pub struct BasePlatform {
    pub info: PlatformInfo,
    pub client: reqwest::Client,
}

impl BasePlatform {
    pub fn new(info: PlatformInfo, client: reqwest::Client) -> Self {
        BasePlatform { info, client }
    }
    pub fn get_nth_path_from_url<'b>(
        &self,
        url: &'b str,
        n: usize,
    ) -> Result<&'b str, UrlParseError> {
        if !url.contains(&self.info.api_domain) {
            return Err(UrlParseError::InvalidFormat {
                url: url.to_string(),
            });
        }

        let path_start = url
            .find(&self.info.api_domain)
            .ok_or(UrlParseError::UnsupportedSite {
                site: self.info.api_domain.to_string(),
            })?
            + self.info.api_domain.len();

        if path_start >= url.len() {
            return Err(UrlParseError::InvalidFormat {
                url: url.to_string(),
            });
        }

        let path = &url[path_start..];
        let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

        segments
            .get(n)
            .copied() // converts &&str to &str
            .filter(|s| !s.is_empty())
            .ok_or(UrlParseError::MissingId {
                url: url.to_string(),
            })
    }
}

#[non_exhaustive]
pub enum PlatformResult {
    FeedSource(FeedSource),
    FeedItem(FeedItem),
}

#[derive(Clone, Debug, Default)]
pub struct FeedItem {
    /// Identifier for this feed item.
    pub id: String,
    /// Identifier for the feed source of this feed item.
    pub source_id: String,
    /// Title/Description of the update, e.g., "Chapter 100", "Episode 12", "My New Video".
    pub title: String,
    /// Url of the item, e.g., "https://mangadex.org/chapter/..."
    pub item_url: String,
    /// Timestamp of the update.
    pub published: DateTime<Utc>,
}

#[derive(Clone, Debug, Default)]
pub struct FeedSource {
    /// Identifier for this feed source.
    pub id: String,
    /// Identifier to get items for this feed source.
    pub items_id: String,
    /// Human readable name/title, e.g., "One Piece", "PewDiePie".
    pub name: String,
    /// Description of the source.
    pub description: String,
    /// Url of the source, e.g., "https://mangadex.org/title/..."
    pub source_url: String,
    /// Cover/Avatar url.
    pub image_url: Option<String>,
}

#[async_trait]
pub trait Platform: Send + Sync {
    /// Fetch latest item of a feed source based on items id.
    async fn fetch_latest(&self, items_id: &str) -> Result<FeedItem, FeedError>;
    /// Fetch feed source information based on source id.
    async fn fetch_source(&self, source_id: &str) -> Result<FeedSource, FeedError>;
    /// Extract source id of a source url.
    fn get_id_from_source_url<'a>(&self, source_url: &'a str) -> Result<&'a str, FeedError>;
    /// Get source url from a source id.
    fn get_source_url_from_id(&self, source_id: &str) -> String;
    fn get_base(&self) -> &BasePlatform;
    fn get_info(&self) -> &PlatformInfo {
        &self.get_base().info
    }
    fn get_id(&self) -> &'_ str {
        &self.get_info().name
    }
    fn extract_error_message(&self, error: &serde_json::Value) -> String {
        let mut parts = Vec::new();

        // Try to extract common API error fields
        if let Some(title) = error.get("title").and_then(|v| v.as_str()) {
            parts.push(format!("title: {}", title));
        }

        if let Some(detail) = error.get("detail").and_then(|v| v.as_str()) {
            parts.push(format!("detail: {}", detail));
        }

        if let Some(status) = error.get("status").and_then(|v| v.as_str()) {
            parts.push(format!("status: {}", status));
        }

        if let Some(code) = error.get("code").and_then(|v| v.as_str()) {
            parts.push(format!("code: {}", code));
        }

        // Fallback to message if available
        if parts.is_empty()
            && let Some(message) = error.get("message").and_then(|v| v.as_str())
        {
            parts.push(format!("message: {}", message));
        }

        // If we still have nothing useful, dump the whole error object
        if parts.is_empty() {
            format!("raw_error: {}", error)
        } else {
            parts.join(", ")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_nth_path_from_url() {
        let info = PlatformInfo {
            name: "Test".to_string(),
            feed_item_name: "Type".to_string(),
            api_hostname: "test.com".to_string(),
            api_domain: "test.com".to_string(),
            api_url: "https://test.com".to_string(),
            ..Default::default()
        };
        let client = reqwest::Client::new();
        let base = BasePlatform::new(info, client);

        let url = "https://test.com/one/two/three";

        assert_eq!(base.get_nth_path_from_url(url, 0).unwrap(), "one");
        assert_eq!(base.get_nth_path_from_url(url, 1).unwrap(), "two");
        assert_eq!(base.get_nth_path_from_url(url, 2).unwrap(), "three");

        // Out of bounds
        assert!(matches!(
            base.get_nth_path_from_url(url, 3),
            Err(UrlParseError::MissingId { .. })
        ));

        // Wrong domain
        let wrong_url = "https://other.com/one";
        assert!(matches!(
            base.get_nth_path_from_url(wrong_url, 0),
            Err(UrlParseError::InvalidFormat { .. })
        ));
    }
}
