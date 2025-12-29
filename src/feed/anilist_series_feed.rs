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
use serde_json::Value;

use crate::feed::BaseFeed;
use crate::feed::Feed;
use crate::feed::FeedInfo;
use crate::feed::FeedItem;
use crate::feed::FeedSource;
use crate::feed::error::SeriesFeedError;
use crate::feed::error::UrlParseError;

pub struct AniListSeriesFeed {
    pub base: BaseFeed,
    limiter: RateLimiter<NotKeyed, InMemoryState, QuantaClock>,
}

impl AniListSeriesFeed {
    pub fn new() -> Self {
        let info = FeedInfo {
            name: "AniList".to_string(),
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
        Self {
            base: BaseFeed::new(info, reqwest::Client::new()),
            limiter,
        }
    }

    async fn request(
        &self,
        source_id: &str,
        query: &str,
    ) -> Result<serde_json::Value, SeriesFeedError> {
        let source_id_num = Self::validate_id(source_id)?;
        let json = serde_json::json!({
            "query": query,
            "variables": { "id": source_id_num }
        });

        let request = self.base.client.post(&self.base.info.api_url).json(&json);
        let response = self.send(request).await?;
        let response_json = response.json::<serde_json::Value>().await?; // Automatically converts to SourceError::JsonParseFailed

        self.check_api_errors(&response_json)?;

        Ok(response_json)
    }

    fn check_api_errors(&self, resp: &Value) -> Result<(), SeriesFeedError> {
        if let Some(errors) = resp.get("errors")
            && let Some(error_array) = errors.as_array()
        {
            let err_msg = error_array
                .iter()
                .map(|e| self.extract_error_message(e))
                .collect::<Vec<String>>()
                .join(" | ");
            return Err(SeriesFeedError::ApiError { message: err_msg });
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

    /// Validate source_id format (should be numeric for AniList)
    fn validate_id(source_id: &str) -> Result<i32, SeriesFeedError> {
        let source_id_num =
            source_id
                .parse::<i32>()
                .map_err(|_| SeriesFeedError::InvalidSourceId {
                    source_id: source_id.to_string(),
                })?;
        Ok(source_id_num)
    }
}

#[async_trait]
impl Feed for AniListSeriesFeed {
    async fn fetch_latest(&self, id: &str) -> Result<FeedItem, SeriesFeedError> {
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

        // Extract fields
        let airing_schedule = response_json["data"]["AiringSchedule"]
            .as_object()
            .ok_or_else(|| SeriesFeedError::ItemNotFound {
                source_id: source_id.clone(),
            })?;

        let timestamp_s =
            airing_schedule
                .get("airingAt")
                .ok_or_else(|| SeriesFeedError::MissingField {
                    field: "data.AiringSchedule.airingAt".to_string(),
                })?;
        let timestamp = timestamp_s
            .as_i64()
            .ok_or_else(|| SeriesFeedError::UnexpectedResult {
                message: format!("Invalid data.airingSchedule.airingAt: {timestamp_s}"),
            })?;

        let title = airing_schedule
            .get("episode")
            .ok_or_else(|| SeriesFeedError::MissingField {
                field: "data.AiringSchedule.episode".to_string(),
            })?
            .to_string();

        let id = airing_schedule
            .get("id")
            .ok_or_else(|| SeriesFeedError::MissingField {
                field: "data.AiringSchedule.id".to_string(),
            })?
            .to_string();

        let published = DateTime::from_timestamp(timestamp, 0)
            .ok_or_else(|| SeriesFeedError::InvalidTimestamp { timestamp })?;

        info!("Successfully fetched anime for source_id: {source_id}");

        Ok(FeedItem {
            id,
            url: self.get_url_from_id(&source_id),
            source_id,
            title,
            published,
        })
    }

    async fn fetch_source(&self, id: &str) -> Result<FeedSource, SeriesFeedError> {
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

        // Extract fields
        let media = response_json["data"]["Media"].as_object().ok_or_else(|| {
            SeriesFeedError::SourceNotFound {
                source_id: source_id.clone(),
            }
        })?;

        let name = media["title"]["romaji"]
            .as_str()
            .unwrap_or("Unknown")
            .to_string();

        let description = media["description"]
            .as_str()
            .unwrap_or("Unknown")
            .to_string();

        let image_url = Some(
            media["coverImage"]["extraLarge"]
                .as_str()
                .unwrap_or("Unknown")
                .to_string(),
        );

        Ok(FeedSource {
            id: source_id,
            name,
            url: self.get_url_from_id(id),
            image_url,
            description,
        })
    }

    fn get_id_from_url<'a>(&self, url: &'a str) -> Result<&'a str, UrlParseError> {
        self.base.get_nth_path_from_url(url, 1)
    }

    fn get_url_from_id(&self, id: &str) -> String {
        format!("https://{}/anime/{}", self.base.info.api_domain, id)
    }

    fn get_base(&self) -> &BaseFeed {
        &self.base
    }
}

impl PartialEq for AniListSeriesFeed {
    fn eq(&self, other: &Self) -> bool {
        self.base.info.api_url == other.base.info.api_url
    }
}

impl Eq for AniListSeriesFeed {}

impl Hash for AniListSeriesFeed {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.base.info.api_url.hash(state);
    }
}
