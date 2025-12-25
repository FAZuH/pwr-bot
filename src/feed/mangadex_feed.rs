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

use super::BaseFeed;
use crate::feed::FeedUrl;
use crate::feed::error::SeriesError;
use crate::feed::error::UrlParseError;
use crate::feed::series::SeriesFeed;
use crate::feed::series::SeriesItem;
use crate::feed::series::SeriesLatest;

type Json<'a> = &'a Map<String, Value>;

pub struct MangaDexFeed<'a> {
    pub base: BaseFeed<'a>,
    limiter: RateLimiter<NotKeyed, InMemoryState, QuantaClock>,
}

impl MangaDexFeed<'_> {
    pub fn new() -> Self {
        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static("pwr-bot/0.1"));

        let client = Client::builder()
            .default_headers(headers)
            .build()
            .expect("Failed to create client");

        let url = FeedUrl {
            name: "MangaDex",
            api_hostname: "api.mangadex.org",
            api_domain: "mangadex.org",
            api_url: "https://api.mangadex.org",
        };
        // NOTE: See https://api.mangadex.org/docs/2-limitations/
        // Because GET /manga/{id} is not specified on #endpoint-specific-rate-limits,
        // therefore GET /manga/{id} has a default ratelimit of 5 requests per second

        let limiter = RateLimiter::direct(Quota::per_second(NonZeroU32::new(5).unwrap()));

        Self {
            base: BaseFeed::new(url, client),
            limiter,
        }
    }

    fn check_resp_errors(&self, resp: &Value) -> Result<(), SeriesError> {
        if let Some(errors) = resp.get("errors")
            && let Some(error_array) = errors.as_array()
            && let Some(first_error) = error_array.first()
        {
            let message = first_error["message"]
                .as_str()
                .unwrap_or("Unknown API error")
                .to_string();
            return Err(SeriesError::ApiError { message });
        }
        Ok(())
    }

    fn get_data_from_resp<'a>(&self, resp: &'a Value) -> Result<&'a Value, SeriesError> {
        resp.get("data").ok_or_else(|| SeriesError::MissingField {
            field: "data".to_string(),
        })
    }

    fn get_attr_from_data<'a>(
        &self,
        data: &'a Value,
    ) -> Result<&'a Map<String, Value>, SeriesError> {
        data["attributes"]
            .as_object()
            .ok_or_else(|| SeriesError::MissingField {
                field: "data".to_string(),
            })
    }

    /// Get title from `/manga/{id}` endpoint response.
    ///
    /// Priority: title.en > altTitles.en > title.ja-ro > altTitles.ja-ro > title.ja > altTitles.ja
    /// I apologize in advance to the future me for this mess
    fn get_title_from_attr(&self, attr: Json) -> Result<String, SeriesError> {
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

        Err(SeriesError::MissingField {
            field: "title or altTitles in en/ja-ro/ja".to_string(),
        })
    }

    fn get_description_from_attr(&self, attr: Json) -> Result<String, SeriesError> {
        Ok(attr["description"]["en"]
            .as_str()
            .ok_or_else(|| SeriesError::MissingField {
                field: "description.en".to_string(),
            })?
            .to_string())
    }

    async fn get_cover_url(&self, id: &str) -> Result<String, SeriesError> {
        debug!(
            "Fetching cover from {} for series_id: {id}",
            self.base.url.name
        );
        let request = self
            .base
            .client
            .get(format!("{}/cover/{id}", self.base.url.api_url));

        let resp = self.send_get_json(request).await?;
        let attr = self.get_attr_from_data(&resp)?;

        let cover_filename = attr
            .get("fileName")
            .ok_or_else(|| SeriesError::MissingField {
                field: "data.attributes.fileName".to_string(),
            })?
            .to_string();

        let ret = format!("https://uploads.mangadex.org/covers/{id}/{cover_filename}");
        Ok(ret)
    }

    fn validate_uuid(&self, uuid: &String) -> Result<(), SeriesError> {
        if uuid::Uuid::parse_str(uuid).is_err() {
            return Err(SeriesError::InvalidSeriesId {
                series_id: uuid.to_string(),
            });
        }
        Ok(())
    }

    async fn send(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, reqwest::Error> {
        if self.limiter.check().is_err() {
            info!("Source {} is ratelimited. Waiting...", self.base.url.name);
        }
        self.limiter.until_ready().await;

        let req = request.build()?;
        debug!("Making request to: {}", req.url());
        self.base.client.execute(req).await
    }

    async fn send_get_json(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<serde_json::Value, SeriesError> {
        let response = self.send(request).await?;

        let body = response.text().await?;
        let resp: serde_json::Value = serde_json::from_str(&body)?;
        self.check_resp_errors(&resp)?;
        Ok(resp)
    }
}

#[async_trait]
impl SeriesFeed for MangaDexFeed<'_> {
    async fn get_info(&self, id: &str) -> Result<SeriesItem, SeriesError> {
        debug!(
            "Fetching info from {} for series_id: {id}",
            self.base.url.name
        );
        let series_id = id.to_string();
        self.validate_uuid(&series_id.clone())?;

        let request = self
            .base
            .client
            .get(format!("{}/manga/{id}", self.base.url.api_url));

        let resp = self.send_get_json(request).await?;
        let data = self.get_data_from_resp(&resp)?;
        let attr = self.get_attr_from_data(&data)?;
        let title = self.get_title_from_attr(attr)?;
        let description = self.get_description_from_attr(attr)?;
        let cover_url = Some(self.get_cover_url(&series_id).await?);

        info!("Successfully fetched latest manga for series_id: {series_id}");

        Ok(SeriesItem {
            title,
            url: self.get_url_from_id(&series_id),
            cover_url,
            id: series_id,
            description,
        })
    }

    async fn get_latest(&self, id: &str) -> Result<SeriesLatest, SeriesError> {
        debug!(
            "Fetching latest from {} for series_id: {id}",
            self.base.url.name
        );
        let series_id = id.to_string();

        let request = self
            .base
            .client
            .get(format!("{}/manga/{series_id}/feed", self.base.url.api_url))
            .query(&[
                ("order[createdAt]", "desc"),
                ("limit", "1"),
                ("translatedLanguage[]", "en"),
                ("translatedLanguage[]", "id"),
            ]);

        let resp = self.send_get_json(request).await?;

        // Extract fields
        let data = self.get_data_from_resp(&resp)?;
        let chapters = data
            .as_array()
            .ok_or_else(|| SeriesError::UnexpectedResult {
                message: "data field is not an array".to_string(),
            })?;

        if let Some(c) = chapters.first() {
            let id = c["id"]
                .as_str()
                .ok_or_else(|| SeriesError::MissingField {
                    field: "data.0.id".to_string(),
                })?
                .to_string();

            let latest = c["attributes"]["chapter"]
                .as_str()
                .ok_or_else(|| SeriesError::MissingField {
                    field: "data.0.attributes.chapter".to_string(),
                })?
                .to_string();

            let publish_at = c["attributes"]["publishAt"]
                .as_str()
                .ok_or_else(|| SeriesError::MissingField {
                    field: "data.0.attributes.publishAt".to_string(),
                })?
                .to_string();

            let published = DateTime::parse_from_rfc3339(&publish_at)
                .map(|dt| dt.with_timezone(&Utc))
                .map_err(|_| SeriesError::InvalidTime { time: publish_at })?;

            info!("Successfully fetched latest manga for series_id: {series_id}");

            Ok(SeriesLatest {
                id,
                url: self.get_url_from_id(&series_id),
                series_id,
                latest,
                published,
            })
        } else {
            warn!("No chapters found in data for series_id: {series_id}");
            Err(SeriesError::EmptySeries {
                series_id: series_id.to_string(),
            })
        }
    }

    fn get_id_from_url<'a>(&self, url: &'a str) -> Result<&'a str, UrlParseError> {
        self.base.get_nth_path_from_url(url, 1)
    }

    fn get_url_from_id(&self, id: &str) -> String {
        format!("https://{}/title/{}", self.base.url.api_domain, id)
    }

    fn get_base(&self) -> &BaseFeed<'_> {
        &self.base
    }
}

impl PartialEq for MangaDexFeed<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.base.url.api_url == other.base.url.api_url
    }
}

impl Eq for MangaDexFeed<'_> {}

impl Hash for MangaDexFeed<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.base.url.api_url.hash(state);
    }
}
