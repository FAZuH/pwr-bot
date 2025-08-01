use std::sync::Arc;

use super::Source;
use super::error::SourceError;
use super::model::SourceResult;

use super::anilist_source::AniListSource;
use super::mangadex_source::MangaDexSource;

pub struct Sources {
    sources: Vec<Arc<dyn Source>>,
    pub anilist_source: Arc<AniListSource<'static>>,
    pub mangadex_source: Arc<MangaDexSource<'static>>,
}

impl Sources {
    pub fn new() -> Self {
        let anilist_source = Arc::new(AniListSource::new());
        let mangadex_source = Arc::new(MangaDexSource::new());

        let mut _self = Self {
            sources: Vec::new(),
            anilist_source,
            mangadex_source,
        };

        _self.add_source(_self.anilist_source.clone());
        _self.add_source(_self.mangadex_source.clone());
        _self
    }

    /// Get series id by URL
    pub fn get_series_id_by_url<'a>(&self, url: &'a str) -> Result<&'a str, SourceError> {
        let source = self
            .get_source_by_url(url)
            .ok_or_else(|| SourceError::UnsupportedUrl {
                url: url.to_string(),
            })?;

        let ret = source.get_id_from_url(url)?;
        Ok(ret)
    }

    /// Get source by URL and call get_latest
    pub async fn get_latest_by_url(&self, url: &str) -> Result<SourceResult, SourceError> {
        let source = self
            .get_source_by_url(url)
            .ok_or_else(|| SourceError::UnsupportedUrl {
                url: url.to_string(),
            })?;
        let series_id = self.get_series_id_by_url(url)?;
        source.get_latest(&series_id).await
    }

    /// Get source by URL
    pub fn get_source_by_url(&self, url: &str) -> Option<&Arc<dyn Source>> {
        self.sources.iter().find(|source| {
            source
                .get_url()
                .api_url
                .contains(&Self::extract_domain(url))
        })
    }

    pub fn add_source(&mut self, source: Arc<dyn Source>) {
        self.sources.push(source);
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

impl Default for Sources {
    fn default() -> Self {
        Self::new()
    }
}
