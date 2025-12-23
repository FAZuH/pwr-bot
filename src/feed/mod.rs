use crate::feed::error::UrlParseError;
use crate::feed::series::SeriesItem;
use crate::feed::series::SeriesLatest;

pub mod anilist_feed;

pub mod error;
pub mod feeds;
pub mod mangadex_feed;
pub mod series;

#[derive(Clone, Debug)]
pub struct FeedUrl<'a> {
    /// The name of the feed, e.g., "MangaDex", "AniList"
    pub name: &'a str,
    /// api.feed.tld
    pub api_hostname: &'a str,
    /// feed.tld
    pub api_domain: &'a str,
    /// https://api.feed.tld
    pub api_url: &'a str,
}

#[derive(Clone)]
pub struct BaseFeed<'a> {
    pub url: FeedUrl<'a>,
    pub client: reqwest::Client,
}

impl<'a> BaseFeed<'a> {
    pub fn new(url: FeedUrl<'a>, client: reqwest::Client) -> Self {
        BaseFeed { url, client }
    }
    pub fn get_nth_path_from_url<'b>(
        &self,
        url: &'b str,
        n: usize,
    ) -> Result<&'b str, UrlParseError> {
        if !url.contains(self.url.api_domain) {
            return Err(UrlParseError::InvalidFormat {
                url: url.to_string(),
            });
        }

        let path_start = url
            .find(self.url.api_domain)
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

#[non_exhaustive]
pub enum FeedResult {
    SeriesItem(SeriesItem),
    SeriesLatest(SeriesLatest),
}
