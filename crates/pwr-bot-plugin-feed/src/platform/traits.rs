use async_trait::async_trait;
use chrono::DateTime;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;

use crate::error::FeedError;
use crate::error::UrlParseError;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PlatformInfo {
    pub name: String,
    pub feed_item_name: String,
    pub api_hostname: String,
    pub api_domain: String,
    pub api_url: String,
    pub copyright_notice: String,
    pub logo_url: String,
    pub tags: String,
}

#[derive(Clone, Debug)]
pub struct BasePlatform {
    pub info: PlatformInfo,
}

impl BasePlatform {
    pub fn new(info: PlatformInfo) -> Self {
        BasePlatform { info }
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
            .copied()
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

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct FeedItem {
    pub id: String,
    pub title: String,
    pub published: DateTime<Utc>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct FeedSource {
    pub id: String,
    pub items_id: String,
    pub name: String,
    pub description: String,
    pub source_url: String,
    pub image_url: Option<String>,
}

#[async_trait]
pub trait Platform: Send + Sync {
    async fn fetch_latest(&self, items_id: &str) -> Result<FeedItem, FeedError>;
    async fn fetch_source(&self, source_id: &str) -> Result<FeedSource, FeedError>;
    fn get_id_from_source_url<'a>(&self, source_url: &'a str) -> Result<&'a str, FeedError>;
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
        if let Some(title) = error.get("title").and_then(|v| v.as_str()) {
            parts.push(format!("title: {title}"));
        }
        if let Some(detail) = error.get("detail").and_then(|v| v.as_str()) {
            parts.push(format!("detail: {detail}"));
        }
        if let Some(status) = error.get("status").and_then(|v| v.as_str()) {
            parts.push(format!("status: {status}"));
        }
        if let Some(code) = error.get("code").and_then(|v| v.as_str()) {
            parts.push(format!("code: {code}"));
        }
        if parts.is_empty()
            && let Some(message) = error.get("message").and_then(|v| v.as_str())
        {
            parts.push(format!("message: {message}"));
        }
        if parts.is_empty() {
            format!("raw_error: {error}")
        } else {
            parts.join(", ")
        }
    }
}
