use std::sync::Arc;

use crate::feed::Platform;
use crate::feed::anilist_platform::AniListPlatform;
use crate::feed::error::FeedError;
use crate::feed::mangadex_platform::MangaDexPlatform;

pub struct Platforms {
    platforms: Vec<Arc<dyn Platform>>,
    pub anilist: Arc<AniListPlatform>,
    pub mangadex: Arc<MangaDexPlatform>,
}

impl Platforms {
    pub fn new() -> Self {
        let anilist = Arc::new(AniListPlatform::new());
        let mangadex = Arc::new(MangaDexPlatform::new());

        let mut _self = Self {
            platforms: Vec::new(),
            anilist,
            mangadex,
        };

        _self.add_platform(_self.anilist.clone());
        _self.add_platform(_self.mangadex.clone());
        _self
    }

    /// Extract source id of a source url.
    pub fn get_id_from_source_url<'a>(&self, source_url: &'a str) -> Result<&'a str, FeedError> {
        let feed = self
            .get_platform_by_source_url(source_url)
            .ok_or_else(|| FeedError::UnsupportedUrl {
                url: source_url.to_string(),
            })?;

        let ret = feed.get_id_from_source_url(source_url)?;
        Ok(ret)
    }

    /// Get platform by source url.
    pub fn get_platform_by_source_url(&self, source_url: &str) -> Option<&Arc<dyn Platform>> {
        self.platforms.iter().find(|feed| {
            feed.get_base()
                .info
                .api_url
                .contains(&Self::extract_domain(source_url))
        })
    }

    pub fn get_all_platforms(&self) -> Vec<Arc<dyn Platform>> {
        self.platforms.clone()
    }

    pub fn add_platform(&mut self, feed: Arc<dyn Platform>) {
        self.platforms.push(feed);
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

impl Default for Platforms {
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
            Platforms::extract_domain("https://example.com/foo/bar"),
            "example.com"
        );
        assert_eq!(Platforms::extract_domain("http://example.com"), "example.com");
        assert_eq!(Platforms::extract_domain("example.com/foo"), "example.com");
        assert_eq!(Platforms::extract_domain("example.com"), "example.com");
        assert_eq!(
            Platforms::extract_domain("https://sub.domain.co.uk/path"),
            "sub.domain.co.uk"
        );
    }
}
