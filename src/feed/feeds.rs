use std::sync::Arc;

use crate::feed::Feed;
use crate::feed::FeedItem;
use crate::feed::FeedSource;
use crate::feed::anilist_series_feed::AniListSeriesFeed;
use crate::feed::error::SeriesFeedError;
use crate::feed::mangadex_series_feed::MangaDexSeriesFeed;

pub struct Feeds {
    feeds: Vec<Arc<dyn Feed>>,
    pub anilist_feed: Arc<AniListSeriesFeed>,
    pub mangadex_feed: Arc<MangaDexSeriesFeed>,
}

impl Feeds {
    pub fn new() -> Self {
        let anilist_feed = Arc::new(AniListSeriesFeed::new());
        let mangadex_feed = Arc::new(MangaDexSeriesFeed::new());

        let mut _self = Self {
            feeds: Vec::new(),
            anilist_feed,
            mangadex_feed,
        };

        _self.add_feed(_self.anilist_feed.clone());
        _self.add_feed(_self.mangadex_feed.clone());
        _self
    }

    /// Get feed id by URL
    pub fn get_feed_id_by_url<'a>(&self, url: &'a str) -> Result<&'a str, SeriesFeedError> {
        let feed = self
            .get_feed_by_url(url)
            .ok_or_else(|| SeriesFeedError::UnsupportedUrl {
                url: url.to_string(),
            })?;

        let ret = feed.get_id_from_url(url)?;
        Ok(ret)
    }

    /// Get feed by URL and call fetch_latest
    pub async fn fetch_latest_by_url(&self, url: &str) -> Result<FeedItem, SeriesFeedError> {
        let feed = self
            .get_feed_by_url(url)
            .ok_or_else(|| SeriesFeedError::UnsupportedUrl {
                url: url.to_string(),
            })?;
        let source_id = self.get_feed_id_by_url(url)?;
        feed.fetch_latest(source_id).await
    }

    /// Get feed by URL and call fetch_source
    pub async fn fetch_source_by_url(&self, url: &str) -> Result<FeedSource, SeriesFeedError> {
        let feed = self
            .get_feed_by_url(url)
            .ok_or_else(|| SeriesFeedError::UnsupportedUrl {
                url: url.to_string(),
            })?;
        let source_id = self.get_feed_id_by_url(url)?;
        feed.fetch_source(source_id).await
    }

    /// Get feed by URL
    pub fn get_feed_by_url(&self, url: &str) -> Option<&Arc<dyn Feed>> {
        self.feeds.iter().find(|feed| {
            feed.get_base()
                .info
                .api_url
                .contains(&Self::extract_domain(url))
        })
    }

    pub fn add_feed(&mut self, feed: Arc<dyn Feed>) {
        self.feeds.push(feed);
    }

    fn extract_domain(url: &str) -> String {
        let after_protocol = if let Some(domain_start) = url.find("://") {
            &url[domain_start + 3..]
        } else {
            url
        };

        if let Some(domain_end) = after_protocol.find('/') {
            after_protocol[..domain_end].to_string()
        } else {
            after_protocol.to_string()
        }
    }
}

impl Default for Feeds {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_domain() {
        assert_eq!(
            Feeds::extract_domain("https://example.com/foo/bar"),
            "example.com"
        );
        assert_eq!(Feeds::extract_domain("http://example.com"), "example.com");
        assert_eq!(Feeds::extract_domain("example.com/foo"), "example.com");
        assert_eq!(Feeds::extract_domain("example.com"), "example.com");
        assert_eq!(
            Feeds::extract_domain("https://sub.domain.co.uk/path"),
            "sub.domain.co.uk"
        );
    }
}
