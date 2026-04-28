//! MangaDex manga platform integration.

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
use serde_json::Map;
use serde_json::Value;
use wreq::Client;
use wreq::header::HeaderMap;
use wreq::header::HeaderValue;
use wreq::header::USER_AGENT;

use crate::feed::BasePlatform;
use crate::feed::FeedItem;
use crate::feed::FeedSource;
use crate::feed::Platform;
use crate::feed::PlatformInfo;
use crate::feed::error::FeedError;

/// MangaDex API platform for manga tracking.
type Json<'a> = &'a Map<String, Value>;

/// MangaDex platform implementation.
pub struct MangaDexPlatform {
    pub base: BasePlatform,
    client: wreq::Client,
    limiter: RateLimiter<NotKeyed, InMemoryState, QuantaClock>,
}

impl MangaDexPlatform {
    /// Creates a new MangaDex platform with rate limiting.
    pub fn new() -> Self {
        // See https://api.mangadex.org/docs/2-limitations/
        // "The request MUST have a User-Agent header, and it must not be spoofed"
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
            base: BasePlatform::new(info),
            client,
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
        data.get("attributes")
            .and_then(|v| v.as_object())
            .ok_or_else(|| FeedError::MissingField {
                field: "data.attributes".to_string(),
            })
    }

    /// Get title from `/manga/{id}` endpoint response.
    ///
    /// Priority: title.en > altTitles.en > title.ja-ro > altTitles.ja-ro > title.ja > altTitles.ja
    /// I apologize in advance to the future me for this mess
    fn get_title_from_attr(&self, attr: Json) -> Result<String, FeedError> {
        let langs = ["en", "ja-ro", "ja"];

        for lang in langs {
            if let Some(title) = attr
                .get("title")
                .and_then(|t| t.get(lang))
                .and_then(|v| v.as_str())
            {
                return Ok(title.to_string());
            }

            if let Some(alt_titles) = attr.get("altTitles").and_then(|v| v.as_array()) {
                for alt_title in alt_titles {
                    if let Some(title) = alt_title.get(lang).and_then(|v| v.as_str()) {
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
        attr.get("description")
            .and_then(|d| d.get("en"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    }

    fn get_relationships_from_data<'a>(
        &self,
        data: &'a Value,
    ) -> Result<&'a Vec<Value>, FeedError> {
        data.get("relationships")
            .and_then(|v| v.as_array())
            .ok_or_else(|| FeedError::MissingField {
                field: "data.relationships".to_string(),
            })
    }

    fn get_cover_filename(&self, data: &Value) -> Result<String, FeedError> {
        let relationships = self.get_relationships_from_data(data)?;

        let cover_art = relationships
            .iter()
            .find(|rel| rel.get("type").and_then(|v| v.as_str()) == Some("cover_art"))
            .ok_or_else(|| FeedError::MissingField {
                field: "cover_art relationship".to_string(),
            })?;

        cover_art
            .get("attributes")
            .and_then(|v| v.as_object())
            .and_then(|attr| attr.get("fileName"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| FeedError::MissingField {
                field: "cover_art.attributes.fileName".to_string(),
            })
    }

    fn get_chapters_from_data<'a>(&self, data: &'a Value) -> Result<&'a Vec<Value>, FeedError> {
        data.as_array().ok_or_else(|| FeedError::UnexpectedResult {
            message: "data field is not an array".to_string(),
        })
    }

    fn get_first_chapter<'a>(
        &self,
        chapters: &'a [Value],
        source_id: &str,
    ) -> Result<&'a Value, FeedError> {
        chapters.first().ok_or_else(|| {
            warn!("No chapters found in data for source_id: {source_id}");
            FeedError::EmptySource {
                source_id: source_id.to_string(),
            }
        })
    }

    fn get_chapter_id(&self, chapter: &Value) -> Result<String, FeedError> {
        chapter
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| FeedError::MissingField {
                field: "chapter.id".to_string(),
            })
            .map(|s| s.to_string())
    }

    fn get_chapter_attributes<'a>(
        &self,
        chapter: &'a Value,
    ) -> Result<&'a Map<String, Value>, FeedError> {
        chapter
            .get("attributes")
            .and_then(|v| v.as_object())
            .ok_or_else(|| FeedError::MissingField {
                field: "chapter.attributes".to_string(),
            })
    }

    fn get_chapter_title(&self, attributes: &Map<String, Value>) -> Result<String, FeedError> {
        attributes
            .get("chapter")
            .and_then(|v| v.as_str())
            .ok_or_else(|| FeedError::MissingField {
                field: "attributes.chapter".to_string(),
            })
            .map(|s| s.to_string())
    }

    fn get_chapter_publish_at(
        &self,
        attributes: &Map<String, Value>,
    ) -> Result<DateTime<Utc>, FeedError> {
        let date_str = attributes
            .get("publishAt")
            .and_then(|v| v.as_str())
            .ok_or_else(|| FeedError::MissingField {
                field: "attributes.publishAt".to_string(),
            })?;

        DateTime::parse_from_rfc3339(date_str)
            .map(|dt| dt.with_timezone(&Utc))
            .map_err(|_| FeedError::InvalidTime {
                time: date_str.to_string(),
            })
    }

    fn validate_uuid(&self, uuid: &String) -> Result<(), FeedError> {
        if uuid::Uuid::parse_str(uuid).is_err() {
            return Err(FeedError::InvalidSourceId {
                source_id: uuid.to_string(),
            });
        }
        Ok(())
    }

    async fn send(&self, request: wreq::RequestBuilder) -> Result<wreq::Response, wreq::Error> {
        if self.limiter.check().is_err() {
            info!("Source {} is ratelimited. Waiting...", self.base.info.name);
        }
        self.limiter.until_ready().await;

        let req = request.build()?;
        debug!("Making request to: {}", req.url());
        self.client.execute(req).await
    }

    async fn send_get_json(
        &self,
        request: wreq::RequestBuilder,
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
    async fn fetch_source(&self, source_id: &str) -> Result<FeedSource, FeedError> {
        debug!(
            "Fetching info from {} for source_id: {source_id}",
            self.base.info.name
        );
        let source_id = source_id.to_string();
        self.validate_uuid(&source_id.clone())?;

        let request = self.client.get(format!(
            "{}/manga/{source_id}?includes[]=cover_art",
            self.base.info.api_url
        ));

        let resp = self.send_get_json(request).await?;
        let data = self.get_data_from_resp(&resp)?;
        let attr = self.get_attr_from_data(data)?;
        let name = self.get_title_from_attr(attr)?;
        let description = self.get_description_from_attr(attr);

        let cover_filename = self.get_cover_filename(data)?;
        let image_url = Some(format!(
            "https://uploads.mangadex.org/covers/{source_id}/{cover_filename}"
        ));
        let source_url = self.get_source_url_from_id(&source_id);

        Ok(FeedSource {
            items_id: source_id.clone(),
            name,
            source_url,
            image_url,
            id: source_id,
            description,
        })
    }

    async fn fetch_latest(&self, items_id: &str) -> Result<FeedItem, FeedError> {
        debug!(
            "Fetching latest from {} for source_id: {items_id}",
            self.base.info.name
        );
        let source_id = items_id.to_string();

        let request = self
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
        let chapters = self.get_chapters_from_data(data)?;
        let chapter = self.get_first_chapter(chapters, &source_id)?;
        let attributes = self.get_chapter_attributes(chapter)?;

        let id = self.get_chapter_id(chapter)?;
        let title = self.get_chapter_title(attributes)?;
        let published = self.get_chapter_publish_at(attributes)?;

        Ok(FeedItem {
            id,
            title,
            published,
        })
    }

    fn get_id_from_source_url<'a>(&self, url: &'a str) -> Result<&'a str, FeedError> {
        Ok(self.base.get_nth_path_from_url(url, 1)?)
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
