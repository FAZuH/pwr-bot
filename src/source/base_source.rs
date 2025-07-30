use crate::source::error::UrlParseError;

#[derive(Clone)]
pub struct BaseSource {
    pub api_domain: String,
    pub api_url: String,
    pub client: reqwest::Client,
}

impl BaseSource {
    pub fn new(api_domain: String, api_url: String, client: reqwest::Client) -> Self {
        BaseSource { api_domain, api_url, client }
    }
    pub fn get_nth_path_from_url(&self, url: &String, n: usize) -> Result<String, super::error::UrlParseError> {
        if !url.contains(&self.api_domain) {
            return Err(UrlParseError::InvalidFormat { url: url.clone() });
        }

        let path_start = url.find(&self.api_domain)
            .ok_or(UrlParseError::UnsupportedSite { site: self.api_domain.clone() })?
            + self.api_domain.len();

        if path_start >= url.len() {
            return Err(UrlParseError::InvalidFormat { url: url.clone() });
        }

        let path = &url[path_start..];
        let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        
        segments.get(n)
            .filter(|s| !s.is_empty())
            .map(|&s| s.to_string())
            .ok_or(UrlParseError::MissingId { url: url.clone() })
    }
}
