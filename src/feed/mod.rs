pub mod anilist_feed;

pub mod error;
pub mod feeds;
pub mod mangadex_feed;

use async_trait::async_trait;
use chrono::DateTime;
use chrono::Utc;

use crate::feed::error::SeriesFeedError;
use crate::feed::error::UrlParseError;

#[derive(Clone, Debug, Default)]
pub struct FeedInfo {
    /// The name of the feed source, e.g., "MangaDex", "AniList"
    pub name: String,
    /// What do you call the item this feed publishes? e.g., "Episode", "Chapter"
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
    /// Feed tags. Mainly used for grouping and filtering
    pub tags: String,
}

#[derive(Clone, Debug)]
pub struct BaseFeed {
    pub info: FeedInfo,
    pub client: reqwest::Client,
}

impl BaseFeed {
    pub fn new(info: FeedInfo, client: reqwest::Client) -> Self {
        BaseFeed { info, client }
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
pub enum FeedResult {
    FeedSource(FeedSource),
    FeedItem(FeedItem),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_nth_path_from_url() {
        let info = FeedInfo {
            name: "Test".to_string(),
            feed_item_name: "Type".to_string(),
            api_hostname: "test.com".to_string(),
            api_domain: "test.com".to_string(),
            api_url: "https://test.com".to_string(),
            ..Default::default()
        };
        let client = reqwest::Client::new();
        let base = BaseFeed::new(info, client);

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

#[derive(Clone, Debug, Default)]
pub struct FeedItem {
    pub id: String,
    pub source_id: String,
    /// Title/Description of the update, e.g., "Chapter 100", "Episode 12", "My New Video".
    pub title: String,
    /// Url of the item, e.g., "https://mangadex.org/chapter/..."
    pub url: String,
    /// Timestamp of the update.
    pub published: DateTime<Utc>,
}

#[derive(Clone, Debug, Default)]
pub struct FeedSource {
    pub id: String,
    /// Human readable name/title, e.g., "One Piece", "PewDiePie".
    pub name: String,
    /// Description of the source.
    pub description: String,
    /// Url of the source, e.g., "https://mangadex.org/title/..."
    pub url: String,
    /// Cover/Avatar url.
    pub image_url: Option<String>,
}

#[async_trait]
pub trait Feed: Send + Sync {
    async fn fetch_latest(&self, id: &str) -> Result<FeedItem, SeriesFeedError>;
    async fn fetch_source(&self, id: &str) -> Result<FeedSource, SeriesFeedError>;
    fn get_id_from_url<'a>(&self, url: &'a str) -> Result<&'a str, UrlParseError>;
    /// Returns the URL for a source given its ID.
    /// The returned URL is the public URL of the source, not the API URL.
    fn get_url_from_id(&self, id: &str) -> String;
    fn get_base(&self) -> &BaseFeed;
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
