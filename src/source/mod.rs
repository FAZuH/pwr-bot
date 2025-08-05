pub mod anilist_source;
pub mod error;
pub mod mangadex_source;
pub mod model;
pub mod sources;

use error::SourceError;
use error::UrlParseError;
use model::SourceResult;

use async_trait::async_trait;

#[derive(Clone, Debug)]
pub struct SourceUrl<'a> {
    /// The name of the source, e.g., "MangaDex", "AniList"
    pub name: &'a str,
    /// api.source.tld
    pub api_hostname: &'a str,
    /// source.tld
    pub api_domain: &'a str,
    /// https://api.source.tld
    pub api_url: &'a str,
}

#[derive(Clone)]
pub struct BaseSource<'a> {
    pub url: SourceUrl<'a>,
    pub client: reqwest::Client,
}

impl<'a> BaseSource<'a> {
    pub fn new(url: SourceUrl<'a>, client: reqwest::Client) -> Self {
        BaseSource { url, client }
    }
    pub fn get_nth_path_from_url<'b>(
        &self,
        url: &'b str,
        n: usize,
    ) -> Result<&'b str, UrlParseError> {
        if !url.contains(&self.url.api_domain) {
            return Err(UrlParseError::InvalidFormat {
                url: url.to_string(),
            });
        }

        let path_start = url
            .find(&self.url.api_domain)
            .ok_or(UrlParseError::UnsupportedSite {
                site: self.url.api_domain.to_string(),
            })?
            + self.url.api_domain.len();

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

#[async_trait]
pub trait Source: Send + Sync {
    async fn get_latest(&self, id: &str) -> Result<SourceResult, SourceError>;
    fn get_id_from_url<'a>(&self, url: &'a str) -> Result<&'a str, UrlParseError>;
    /// Returns the URL for a series given its ID.
    /// The returned URL is the public URL of the series, not the API URL.
    fn get_url_from_id(&self, id: &str) -> String;
    fn get_base(&self) -> &BaseSource;
    fn get_url(&self) -> &SourceUrl {
        &self.get_base().url
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
        if parts.is_empty() {
            if let Some(message) = error.get("message").and_then(|v| v.as_str()) {
                parts.push(format!("message: {}", message));
            }
        }

        // If we still have nothing useful, dump the whole error object
        if parts.is_empty() {
            format!("raw_error: {}", error)
        } else {
            parts.join(", ")
        }
    }
}
