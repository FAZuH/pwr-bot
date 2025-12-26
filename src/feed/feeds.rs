use std::sync::Arc;

use crate::feed::anilist_series_feed::AniListSeriesFeed;
use crate::feed::error::SeriesError;
use crate::feed::mangadex_series_feed::MangaDexSeriesFeed;
use crate::feed::series_feed::SeriesFeed;
use crate::feed::series_feed::SeriesItem;
use crate::feed::series_feed::SeriesLatest;

pub struct Feeds {
    feeds: Vec<Arc<dyn SeriesFeed>>,
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
    pub fn get_feed_id_by_url<'a>(&self, url: &'a str) -> Result<&'a str, SeriesError> {
        let feed = self
            .get_feed_by_url(url)
            .ok_or_else(|| SeriesError::UnsupportedUrl {
                url: url.to_string(),
            })?;

        let ret = feed.get_id_from_url(url)?;
        Ok(ret)
    }

    /// Get series feed by URL and call get_latest
    pub async fn get_latest_by_url(&self, url: &str) -> Result<SeriesLatest, SeriesError> {
        let feed = self
            .get_feed_by_url(url)
            .ok_or_else(|| SeriesError::UnsupportedUrl {
                url: url.to_string(),
            })?;
        let series_id = self.get_feed_id_by_url(url)?;
        feed.get_latest(series_id).await
    }

    /// Get series feed by URL and call get_info
    pub async fn get_info_by_url(&self, url: &str) -> Result<SeriesItem, SeriesError> {
        let feed = self
            .get_feed_by_url(url)
            .ok_or_else(|| SeriesError::UnsupportedUrl {
                url: url.to_string(),
            })?;
        let series_id = self.get_feed_id_by_url(url)?;
        feed.get_info(series_id).await
    }

    /// Get series feed by URL
    pub fn get_feed_by_url(&self, url: &str) -> Option<&Arc<dyn SeriesFeed>> {
        self.feeds.iter().find(|feed| {
            feed.get_base()
                .info
                .api_url
                .contains(&Self::extract_domain(url))
        })
    }

    pub fn add_feed(&mut self, feed: Arc<dyn SeriesFeed>) {
        self.feeds.push(feed);
    }

    fn extract_domain(url: &str) -> String {
        if let Some(domain_start) = url.find("://") {
            let after_protocol = &url[domain_start + 3..];
            if let Some(domain_end) = after_protocol.find('/') {
                after_protocol[..domain_end].to_string()
            } else {
                after_protocol.to_string()
            }
        } else {
            url.to_string()
        }
    }
}

impl Default for Feeds {
    fn default() -> Self {
        Self::new()
    }
}
