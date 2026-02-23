//! AniList anime platform integration.

use std::hash::Hash;
use std::hash::Hasher;
use std::num::NonZeroU32;

use async_trait::async_trait;
use chrono::DateTime;
use governor::Quota;
use governor::RateLimiter;
use governor::clock::QuantaClock;
use governor::state::InMemoryState;
use governor::state::direct::NotKeyed;
use log::debug;
use log::info;
use serde_json::Map;
use serde_json::Value;

use crate::feed::BasePlatform;
use crate::feed::FeedItem;
use crate::feed::FeedSource;
use crate::feed::Platform;
use crate::feed::PlatformInfo;
use crate::feed::error::FeedError;

/// AniList GraphQL API platform for anime tracking.
pub struct AniListPlatform {
    pub base: BasePlatform,
    client: wreq::Client,
    limiter: RateLimiter<NotKeyed, InMemoryState, QuantaClock>,
}

impl AniListPlatform {
    /// Creates a new AniList platform with rate limiting.
    pub fn new() -> Self {
        let info = PlatformInfo {
            name: "AniList Anime".to_string(),
            feed_item_name: "Episode".to_string(),
            api_hostname: "graphql.anilist.co".to_string(),
            api_domain: "anilist.co".to_string(),
            api_url: "https://graphql.anilist.co".to_string(),
            copyright_notice: "© AniList LLC 2025".to_string(),
            logo_url: "https://anilist.co/img/icons/android-chrome-192x192.png".to_string(),
            tags: "series".to_string(),
        };
        // TODO: See https://docs.anilist.co/guide/rate-limiting.
        // "The API is currently in a degraded state and is limited to 30 requests per minute."
        // We will use the ratelimit headers `X-RateLimit-Limit` and `X-RateLimit-Remaining` when
        // the API is fully restored.
        let limiter = RateLimiter::direct(Quota::per_minute(NonZeroU32::new(30).unwrap()));
        let client = wreq::Client::builder()
            .emulation(wreq_util::Emulation::Chrome137)
            .build()
            .unwrap();

        Self {
            base: BasePlatform::new(info),
            client,
            limiter,
        }
    }

    async fn request(&self, source_id: &str, query: &str) -> Result<serde_json::Value, FeedError> {
        let source_id_num = Self::validate_id(source_id)?;
        let json = serde_json::json!({
            "query": query,
            "variables": { "id": source_id_num }
        });

        let request = self
            .client
            .post(&self.base.info.api_url)
            .body(json.to_string());
        let response = self.send(request).await?;
        let body = response.text().await?;
        let response_json: serde_json::Value = serde_json::from_str(&body)?;

        self.check_api_errors(&response_json)?;

        Ok(response_json)
    }

    fn check_api_errors(&self, resp: &Value) -> Result<(), FeedError> {
        if let Some(errors) = resp.get("errors")
            && let Some(error_array) = errors.as_array()
        {
            let err_msg = error_array
                .iter()
                .map(|e| self.extract_error_message(e))
                .collect::<Vec<String>>()
                .join(" | ");
            return Err(FeedError::ApiError { message: err_msg });
        }
        Ok(())
    }

    fn get_airing_schedule<'a>(
        &self,
        resp: &'a Value,
        source_id: &str,
    ) -> Result<&'a Map<String, Value>, FeedError> {
        resp.get("data")
            .and_then(|d| d.get("AiringSchedule"))
            .and_then(|v| v.as_object())
            .ok_or_else(|| FeedError::ItemNotFound {
                source_id: source_id.to_string(),
            })
    }

    fn get_timestamp(&self, schedule: &Map<String, Value>) -> Result<i64, FeedError> {
        let ts_val = schedule
            .get("airingAt")
            .ok_or_else(|| FeedError::MissingField {
                field: "data.AiringSchedule.airingAt".to_string(),
            })?;
        ts_val.as_i64().ok_or_else(|| FeedError::UnexpectedResult {
            message: format!("Invalid data.airingSchedule.airingAt: {ts_val}"),
        })
    }

    fn get_episode(&self, schedule: &Map<String, Value>) -> Result<String, FeedError> {
        Ok(schedule
            .get("episode")
            .ok_or_else(|| FeedError::MissingField {
                field: "data.AiringSchedule.episode".to_string(),
            })?
            .to_string())
    }

    fn get_id(&self, schedule: &Map<String, Value>) -> Result<String, FeedError> {
        Ok(schedule
            .get("id")
            .ok_or_else(|| FeedError::MissingField {
                field: "data.AiringSchedule.id".to_string(),
            })?
            .to_string())
    }

    fn get_media<'a>(
        &self,
        resp: &'a Value,
        source_id: &str,
    ) -> Result<&'a Map<String, Value>, FeedError> {
        resp.get("data")
            .and_then(|d| d.get("Media"))
            .and_then(|v| v.as_object())
            .ok_or_else(|| FeedError::SourceNotFound {
                source_id: source_id.to_string(),
            })
    }

    fn get_title_romaji(&self, media: &Map<String, Value>) -> Result<String, FeedError> {
        media
            .get("title")
            .and_then(|t| t.get("romaji"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| FeedError::MissingField {
                field: "data.Media.title.romaji".to_string(),
            })
    }

    fn get_description(&self, media: &Map<String, Value>) -> Result<String, FeedError> {
        media
            .get("description")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| FeedError::MissingField {
                field: "data.Media.description".to_string(),
            })
    }

    fn get_cover_image(&self, media: &Map<String, Value>) -> Result<String, FeedError> {
        media
            .get("coverImage")
            .and_then(|c| c.get("extraLarge"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| FeedError::MissingField {
                field: "data.Media.coverImage.extraLarge".to_string(),
            })
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

    /// Validate source_id format (should be numeric for AniList)
    fn validate_id(source_id: &str) -> Result<i32, FeedError> {
        let source_id_num = source_id
            .parse::<i32>()
            .map_err(|_| FeedError::InvalidSourceId {
                source_id: source_id.to_string(),
            })?;
        Ok(source_id_num)
    }
}

#[async_trait]
impl Platform for AniListPlatform {
    async fn fetch_latest(&self, id: &str) -> Result<FeedItem, FeedError> {
        debug!(
            "Fetching latest from {} for source_id: {id}",
            self.base.info.name
        );
        let source_id = id.to_string();

        let query = r#"
        query ($id: Int) {
          AiringSchedule(mediaId: $id, sort: EPISODE_DESC, notYetAired: false) {
            airingAt
            episode
            id
          }
        }
        "#;
        let response_json = self.request(&source_id, query).await?;

        let airing_schedule = self.get_airing_schedule(&response_json, &source_id)?;
        let timestamp = self.get_timestamp(airing_schedule)?;
        let title = self.get_episode(airing_schedule)?;
        let id = self.get_id(airing_schedule)?;

        let published = DateTime::from_timestamp(timestamp, 0)
            .ok_or_else(|| FeedError::InvalidTimestamp { timestamp })?;

        Ok(FeedItem {
            id,
            title,
            published,
        })
    }

    async fn fetch_source(&self, id: &str) -> Result<FeedSource, FeedError> {
        debug!(
            "Fetching info from {} for source_id: {id}",
            self.base.info.name
        );
        let source_id = id.to_string();

        let query = r#"
            query ($id: Int) {
              Media(id: $id, type: ANIME) {
                title { romaji }
                description(asHtml: false)
                coverImage {
                    extraLarge
                }
              }
            }
        "#;
        let response_json = self.request(&source_id, query).await?;

        let media = self.get_media(&response_json, &source_id)?;
        let name = self.get_title_romaji(media)?;
        let description = self.get_description(media)?;
        let image_url = Some(self.get_cover_image(media)?);

        Ok(FeedSource {
            id: source_id.clone(),
            items_id: source_id.clone(),
            name,
            description,
            source_url: self.get_source_url_from_id(id),
            image_url,
        })
    }

    fn get_id_from_source_url<'a>(&self, url: &'a str) -> Result<&'a str, FeedError> {
        Ok(self.base.get_nth_path_from_url(url, 1)?)
    }

    fn get_source_url_from_id(&self, id: &str) -> String {
        format!("https://{}/anime/{}", self.base.info.api_domain, id)
    }

    fn get_base(&self) -> &BasePlatform {
        &self.base
    }
}

impl PartialEq for AniListPlatform {
    fn eq(&self, other: &Self) -> bool {
        self.base.info.api_url == other.base.info.api_url
    }
}

impl Eq for AniListPlatform {}

impl Hash for AniListPlatform {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.base.info.api_url.hash(state);
    }
}
