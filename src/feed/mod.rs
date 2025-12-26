use crate::feed::error::UrlParseError;
use crate::feed::series::SeriesItem;
use crate::feed::series::SeriesLatest;

pub mod anilist_feed;

pub mod error;
pub mod feeds;
pub mod mangadex_feed;
pub mod series;

#[derive(Clone, Debug)]
pub struct FeedInfo {
    /// The name of the feed source, e.g., "MangaDex", "AniList"
    pub name: String,
    /// What do you call the item this feed publishes? e.g., "Episode", "Chapter"
    pub feed_type: String,
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
    SeriesItem(SeriesItem),
    SeriesLatest(SeriesLatest),
}
