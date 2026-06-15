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

use crate::error::FeedError;
use crate::platform::traits::BasePlatform;
use crate::platform::traits::FeedItem;
use crate::platform::traits::FeedSource;
use crate::platform::traits::Platform;
use crate::platform::traits::PlatformInfo;

/// AniList GraphQL API platform for anime tracking.
pub struct AniListPlatform {
    pub base: BasePlatform,
    client: wreq::Client,
    limiter: RateLimiter<NotKeyed, InMemoryState, QuantaClock>,
}

impl Default for AniListPlatform {
    fn default() -> Self {
        Self::new()
    }
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
            .header("Content-Type", "application/json")
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

    pub(crate) fn parse_latest_response(
        &self,
        resp: Value,
        source_id: &str,
    ) -> Result<FeedItem, FeedError> {
        let schedule = self.get_airing_schedule(&resp, source_id)?;
        let timestamp = self.get_timestamp(schedule)?;
        let title = self.get_episode(schedule)?;
        let id = self.get_id(schedule)?;
        let published = DateTime::from_timestamp(timestamp, 0)
            .ok_or_else(|| FeedError::InvalidTimestamp { timestamp })?;
        Ok(FeedItem {
            id,
            title,
            published,
        })
    }

    pub(crate) fn parse_source_response(
        &self,
        resp: Value,
        source_id: &str,
    ) -> Result<FeedSource, FeedError> {
        let media = self.get_media(&resp, source_id)?;
        let name = self.get_title_romaji(media)?;
        let description = self.get_description(media)?;
        let image_url = Some(self.get_cover_image(media)?);
        Ok(FeedSource {
            id: source_id.to_string(),
            items_id: source_id.to_string(),
            name,
            description,
            source_url: self.get_source_url_from_id(source_id),
            image_url,
        })
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

        self.parse_latest_response(response_json, &source_id)
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

        self.parse_source_response(response_json, &source_id)
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    macro_rules! load_fixture {
        ($file:expr) => {
            serde_json::from_str(include_str!(concat!("../../tests/fixtures/", $file))).unwrap()
        };
    }

    fn platform() -> AniListPlatform {
        AniListPlatform::new()
    }

    #[test]
    fn parse_latest_returns_feed_item() {
        let json = load_fixture!("anilist_fetch_latest_exist.json");
        let item = platform().parse_latest_response(json, "173692").unwrap();
        assert_eq!(item.title, "12");
        assert_eq!(item.id, "401043");
        assert_eq!(
            item.published,
            DateTime::from_timestamp(1766327400, 0).unwrap()
        );
    }

    #[test]
    fn parse_latest_with_null_schedule_returns_item_not_found() {
        let json = json!({"data": {"AiringSchedule": null}});
        let result = platform().parse_latest_response(json, "173692");
        assert!(matches!(result, Err(FeedError::ItemNotFound { .. })));
    }

    #[test]
    fn parse_source_returns_feed_source() {
        let json = load_fixture!("anilist_fetch_source_exist.json");
        let result = platform().parse_source_response(json, "173692").unwrap();
        assert_eq!(result.id, "173692");
        assert_eq!(result.items_id, "173692");
        assert_eq!(
            result.name,
            "Chichi wa Eiyuu, Haha wa Seirei, Musume no Watashi wa Tenseisha."
        );
        assert!(result.description.starts_with("Ellen, an 8-year-old girl"));
        assert_eq!(
            result.image_url.as_deref(),
            Some(
                "https://s4.anilist.co/file/anilistcdn/media/anime/cover/large/bx173692-shp7PGRQyCQl.jpg"
            )
        );
        assert_eq!(result.source_url, "https://anilist.co/anime/173692");
    }

    #[test]
    fn parse_source_with_null_media_returns_source_not_found() {
        let json = json!({"data": null});
        let result = platform().parse_source_response(json, "999999");
        assert!(matches!(result, Err(FeedError::SourceNotFound { .. })));
    }

    #[test]
    fn check_api_errors_detects_errors() {
        let json = load_fixture!("anilist_fetch_latest_not_exist.json");
        let result = AniListPlatform::new().check_api_errors(&json);
        assert!(matches!(result, Err(FeedError::ApiError { .. })));
    }

    #[test]
    fn check_api_errors_ok_on_valid() {
        let json = load_fixture!("anilist_fetch_latest_exist.json");
        let result = AniListPlatform::new().check_api_errors(&json);
        assert!(result.is_ok());
    }

    #[test]
    fn validate_id_rejects_non_numeric() {
        let result = AniListPlatform::validate_id("abc");
        assert!(matches!(result, Err(FeedError::InvalidSourceId { .. })));
    }

    #[test]
    fn validate_id_accepts_numeric() {
        let result = AniListPlatform::validate_id("12345");
        assert_eq!(result.unwrap(), 12345);
    }
}
