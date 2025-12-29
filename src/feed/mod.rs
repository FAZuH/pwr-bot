use crate::feed::error::UrlParseError;
use crate::feed::series_feed::SeriesItem;
use crate::feed::series_feed::SeriesLatest;

pub mod anilist_series_feed;

pub mod error;
pub mod feeds;
pub mod mangadex_series_feed;
pub mod series_feed;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_nth_path_from_url() {
        let info = FeedInfo {
            name: "Test".to_string(),
            feed_type: "Type".to_string(),
            api_hostname: "test.com".to_string(),
            api_domain: "test.com".to_string(),
            api_url: "https://test.com".to_string(),
            copyright_notice: "".to_string(),
            logo_url: "".to_string(),
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
