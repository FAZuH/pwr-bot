use async_trait::async_trait;
use chrono::DateTime;
use chrono::Utc;

use crate::feed::BaseFeed;
use crate::feed::error::SeriesError;
use crate::feed::error::UrlParseError;

#[derive(Clone, Debug, Default)]
pub struct SeriesLatest {
    pub id: String,
    pub series_id: String,
    /// Latest chapter or episode title, e.g., "Chapter 100", "Episode 12".
    pub latest: String,
    /// Url of the chapter or episode, e.g., "https://mangadex.org/chapter/1234567890"
    pub url: String,
    /// Timestamp of the latest update, e.g., "2023-10-01T12:00:00Z".
    pub published: DateTime<Utc>,
}

#[derive(Clone, Debug, Default)]
pub struct SeriesItem {
    pub id: String,
    ///  Human readable title, e.g., "One Piece", "Attack on Titan".
    pub title: String,
    /// Description of the series.
    pub description: String,
    /// Url of the series, e.g., "https://mangadex.org/title/1234567890".
    pub url: String,
    /// Cover url of the series.
    pub cover_url: Option<String>,
}

#[async_trait]
pub trait SeriesFeed: Send + Sync {
    async fn get_latest(&self, id: &str) -> Result<SeriesLatest, SeriesError>;
    async fn get_info(&self, id: &str) -> Result<SeriesItem, SeriesError>;
    fn get_id_from_url<'a>(&self, url: &'a str) -> Result<&'a str, UrlParseError>;
    /// Returns the URL for a series given its ID.
    /// The returned URL is the public URL of the series, not the API URL.
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
