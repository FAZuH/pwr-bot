use std::hash::Hash;
use std::hash::Hasher;
use std::num::NonZeroU32;

use async_trait::async_trait;
use chrono::DateTime;
use chrono::Utc;
use governor::Quota;
use governor::RateLimiter;
use governor::clock::QuantaClock;
use governor::state::InMemoryState;
use governor::state::direct::NotKeyed;
use log::debug;
use log::info;
use log::warn;
use reqwest;
use reqwest::Client;
use reqwest::header::HeaderMap;
use reqwest::header::HeaderValue;
use reqwest::header::USER_AGENT;
use serde_json::Map;
use serde_json::Value;

use crate::feed::BasePlatform;
use crate::feed::Platform;
use crate::feed::PlatformInfo;
use crate::feed::FeedItem;
use crate::feed::FeedSource;
use crate::feed::error::FeedError;
use crate::feed::error::UrlParseError;

type Json<'a> = &'a Map<String, Value>;

pub struct MangaDexPlatform {
    pub base: BasePlatform,
    limiter: RateLimiter<NotKeyed, InMemoryState, QuantaClock>,
}

impl MangaDexPlatform {
    pub fn new() -> Self {
        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static("pwr-bot/0.1"));

        let client = Client::builder()
            .default_headers(headers)
            .build()
            .expect("Failed to create client");

        let info = PlatformInfo {
            name: "MangaDex".to_string(),
            feed_item_name: "Chapter".to_string(),
            api_hostname: "api.mangadex.org".to_string(),
            api_domain: "mangadex.org".to_string(),
            api_url: "https://api.mangadex.org".to_string(),
            copyright_notice: "© MangaDex 2025".to_string(),
            // Discord doesn't support .svg files on their embed, and I can't find a .png link
            // under MangaDex's domain
            logo_url: "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/png/manga-dex.png"
                .to_string(),
            tags: "series".to_string(),
        };
        // NOTE: See https://api.mangadex.org/docs/2-limitations/
        // Because GET /manga/{id} is not specified on #endpoint-specific-rate-limits,
        // therefore GET /manga/{id} has a default ratelimit of 5 requests per second

        let limiter = RateLimiter::direct(Quota::per_second(NonZeroU32::new(5).unwrap()));

        Self {
            base: BasePlatform::new(info, client),
            limiter,
        }
    }

    fn check_resp_errors(&self, resp: &Value) -> Result<(), FeedError> {
        if let Some(errors) = resp.get("errors")
            && let Some(error_array) = errors.as_array()
            && let Some(first_error) = error_array.first()
        {
            let message = first_error
                .get("detail")
                .and_then(|v| v.as_str())
                .or_else(|| first_error.get("title").and_then(|v| v.as_str()))
                .unwrap_or("Unknown API error")
                .to_string();

            return Err(FeedError::ApiError { message });
        }
        Ok(())
    }

    fn get_data_from_resp<'a>(&self, resp: &'a Value) -> Result<&'a Value, FeedError> {
        resp.get("data").ok_or_else(|| FeedError::MissingField {
            field: "data".to_string(),
        })
    }

    fn get_attr_from_data<'a>(&self, data: &'a Value) -> Result<&'a Map<String, Value>, FeedError> {
        data["attributes"]
            .as_object()
            .ok_or_else(|| FeedError::MissingField {
                field: "data".to_string(),
            })
    }

    /// Get title from `/manga/{id}` endpoint response.
    ///
    /// Priority: title.en > altTitles.en > title.ja-ro > altTitles.ja-ro > title.ja > altTitles.ja
    /// I apologize in advance to the future me for this mess
    fn get_title_from_attr(&self, attr: Json) -> Result<String, FeedError> {
        let langs = ["en", "ja-ro", "ja"];

        for lang in langs {
            if let Some(title) = attr["title"][lang].as_str() {
                return Ok(title.to_string());
            }

            if let Some(alt_titles) = attr["altTitles"].as_array() {
                for alt_title in alt_titles {
                    if let Some(title) = alt_title[lang].as_str() {
                        return Ok(title.to_string());
                    }
                }
            }
        }

        Err(FeedError::MissingField {
            field: "title or altTitles in en/ja-ro/ja".to_string(),
        })
    }

    fn get_description_from_attr(&self, attr: Json) -> String {
        attr["description"]["en"].as_str().unwrap_or("").to_string()
    }

    async fn get_cover_url(&self, manga_id: &str, data: &Value) -> Result<String, FeedError> {
        let relationships = data
            .get("relationships")
            .and_then(|v| v.as_array())
            .ok_or_else(|| FeedError::MissingField {
                field: "data.relationships".to_string(),
            })?;

        let cover_art = relationships
            .iter()
            .find(|rel| rel.get("type").and_then(|v| v.as_str()) == Some("cover_art"))
            .ok_or_else(|| FeedError::MissingField {
                field: "cover_art relationship".to_string(),
            })?;

        let cover_filename = cover_art
            .get("attributes")
            .and_then(|v| v.as_object())
            .and_then(|attr| attr.get("fileName"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| FeedError::MissingField {
                field: "cover_art.attributes.fileName".to_string(),
            })?;

        Ok(format!(
            "https://uploads.mangadex.org/covers/{manga_id}/{cover_filename}"
        ))
    }

    fn validate_uuid(&self, uuid: &String) -> Result<(), FeedError> {
        if uuid::Uuid::parse_str(uuid).is_err() {
            return Err(FeedError::InvalidSourceId {
                source_id: uuid.to_string(),
            });
        }
        Ok(())
    }

    async fn send(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, reqwest::Error> {
        if self.limiter.check().is_err() {
            info!("Source {} is ratelimited. Waiting...", self.base.info.name);
        }
        self.limiter.until_ready().await;

        let req = request.build()?;
        debug!("Making request to: {}", req.url());
        self.base.client.execute(req).await
    }

    async fn send_get_json(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<serde_json::Value, FeedError> {
        let response = self.send(request).await?;

        let body = response.text().await?;
        let resp: serde_json::Value = serde_json::from_str(&body)?;
        self.check_resp_errors(&resp)?;
        Ok(resp)
    }
}

#[async_trait]
impl Platform for MangaDexPlatform {
    async fn fetch_source(&self, id: &str) -> Result<FeedSource, FeedError> {
        debug!(
            "Fetching info from {} for source_id: {id}",
            self.base.info.name
        );
        let source_id = id.to_string();
        self.validate_uuid(&source_id.clone())?;

        let request = self.base.client.get(format!(
            "{}/manga/{id}?includes[]=cover_art",
            self.base.info.api_url
        ));

        let resp = self.send_get_json(request).await?;
        let data = self.get_data_from_resp(&resp)?;
        let attr = self.get_attr_from_data(data)?;
        let name = self.get_title_from_attr(attr)?;
        let description = self.get_description_from_attr(attr);
        let image_url = Some(self.get_cover_url(&source_id, data).await?);

        info!("Successfully fetched latest manga for source_id: {source_id}");

        Ok(FeedSource {
            name,
            url: self.get_source_url_from_id(&source_id),
            image_url,
            id: source_id,
            description,
        })
    }

    async fn fetch_latest(&self, id: &str) -> Result<FeedItem, FeedError> {
        debug!(
            "Fetching latest from {} for source_id: {id}",
            self.base.info.name
        );
        let source_id = id.to_string();

        let request = self
            .base
            .client
            .get(format!("{}/manga/{source_id}/feed", self.base.info.api_url))
            .query(&[
                ("order[createdAt]", "desc"),
                ("limit", "1"),
                ("translatedLanguage[]", "en"),
                ("translatedLanguage[]", "id"),
            ]);

        let resp = self.send_get_json(request).await?;

        // Extract fields
        let data = self.get_data_from_resp(&resp)?;
        let chapters = data.as_array().ok_or_else(|| FeedError::UnexpectedResult {
            message: "data field is not an array".to_string(),
        })?;

        if let Some(c) = chapters.first() {
            let id = c["id"]
                .as_str()
                .ok_or_else(|| FeedError::MissingField {
                    field: "data.0.id".to_string(),
                })?
                .to_string();

            let title = c["attributes"]["chapter"]
                .as_str()
                .ok_or_else(|| FeedError::MissingField {
                    field: "data.0.attributes.chapter".to_string(),
                })?
                .to_string();

            let publish_at = c["attributes"]["publishAt"]
                .as_str()
                .ok_or_else(|| FeedError::MissingField {
                    field: "data.0.attributes.publishAt".to_string(),
                })?
                .to_string();

            let published = DateTime::parse_from_rfc3339(&publish_at)
                .map(|dt| dt.with_timezone(&Utc))
                .map_err(|_| FeedError::InvalidTime { time: publish_at })?;

            info!("Successfully fetched latest manga for source_id: {source_id}");

            Ok(FeedItem {
                id,
                url: self.get_source_url_from_id(&source_id),
                source_id,
                title,
                published,
            })
        } else {
            warn!("No chapters found in data for source_id: {source_id}");
            Err(FeedError::EmptySource {
                source_id: source_id.to_string(),
            })
        }
    }

    fn get_id_from_source_url<'a>(&self, url: &'a str) -> Result<&'a str, UrlParseError> {
        self.base.get_nth_path_from_url(url, 1)
    }

    fn get_source_url_from_id(&self, id: &str) -> String {
        format!("https://{}/title/{}", self.base.info.api_domain, id)
    }

    fn get_base(&self) -> &BasePlatform {
        &self.base
    }
}

impl PartialEq for MangaDexPlatform {
    fn eq(&self, other: &Self) -> bool {
        self.base.info.api_url == other.base.info.api_url
    }
}

impl Eq for MangaDexPlatform {}

impl Hash for MangaDexPlatform {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.base.info.api_url.hash(state);
    }
}
