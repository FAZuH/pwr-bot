use async_trait::async_trait;
use governor::clock::QuantaClock;
use log::debug;
use reqwest;
use serde_json::Map;
use serde_json::Value;

use crate::source::model::Series;

use super::BaseSource;
use super::SeriesItem;
use super::Source;
use super::SourceResult;
use super::SourceUrl;
use super::error::SourceError;
use super::error::UrlParseError;
use chrono::{DateTime, Utc};
use log::{info, warn};
use reqwest::Client;
use reqwest::header::{HeaderMap, HeaderValue, USER_AGENT};
use std::hash::{Hash, Hasher};

use governor::{
    Quota, RateLimiter,
    state::{InMemoryState, direct::NotKeyed},
};
use std::num::NonZeroU32;

type Data<'a> = &'a Map<String, Value>;

pub struct MangaDexSource<'a> {
    pub base: BaseSource<'a>,
    limiter: RateLimiter<NotKeyed, InMemoryState, QuantaClock>,
}

impl MangaDexSource<'_> {
    pub fn new() -> Self {
        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static("pwr-bot/0.1"));

        let client = Client::builder()
            .default_headers(headers)
            .build()
            .expect("Failed to create client");

        let url = SourceUrl {
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
            base: BaseSource::new(url, client),
            limiter,
        }
    }

    fn check_resp_errors(resp: &Value) -> Result<(), SourceError> {
        if let Some(errors) = resp.get("errors") {
            if let Some(error_array) = errors.as_array() {
                if let Some(first_error) = error_array.first() {
                    let message = first_error["message"]
                        .as_str()
                        .unwrap_or("Unknown API error")
                        .to_string();
                    return Err(SourceError::ApiError { message });
                }
            }
        }
        Ok(())
    }

    fn get_data_from_resp<'a>(
        resp: &'a Value,
        series_id: &'a str,
    ) -> Result<&'a Map<String, Value>, SourceError> {
        resp["data"]
            .as_object()
            .ok_or_else(|| SourceError::SeriesNotFound {
                series_id: series_id.to_string(),
            })
    }

    fn get_title_from_data(data: Data) -> Result<String, SourceError> {
        Ok(data["attributes"]["title"]["en"]
            .as_str()
            .or_else(|| data["attributes"]["title"]["ja"].as_str())
            .ok_or_else(|| SourceError::MissingField {
                field: "title.en or title.ja".to_string(),
            })?
            .to_string())
    }

    fn get_description_from_data(data: Data) -> Result<String, SourceError> {
        Ok(data["attributes"]["description"]["en"]
            .as_str()
            .ok_or_else(|| SourceError::MissingField {
                field: "description.en".to_string(),
            })?
            .to_string())
    }

    fn validate_uuid(&self, uuid: &String) -> Result<(), SourceError> {
        if uuid::Uuid::parse_str(uuid).is_err() {
            return Err(SourceError::InvalidSeriesId {
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
}

#[async_trait]
impl SeriesSource for MangaDexSource<'_> {
    async fn get_info(&self, series_id: &str) -> Result<Series, SourceError> {
        self.validate_uuid(&series_id.to_string().clone())?;

        let request = self
            .base
            .client
            .get(format!("{}/manga", self.base.url.api_url));

        let response = self.send(request).await?; // Converts to SourceError::RequestFailed

        let body = response.text().await?;
        let response_json: serde_json::Value = serde_json::from_str(&body)?; // Converts to SourceError::JsonParseFailed

        Self::check_resp_errors(&response_json)?;

        let data = Self::get_data_from_resp(&response_json, series_id)?;
        let title = Self::get_title_from_data(&data)?;
        let description = Self::get_description_from_data(&data)?;

        info!(
            "Successfully fetched latest manga for series_id: {}",
            series_id
        );

        Ok(Series {
            id: series_id.to_string(),
            title,
            url: self.get_url_from_id(series_id),
            description,
        })
    }

    async fn get_latest(&self, series_id: &str) -> Result<SourceResult, SourceError> {
        let title = self.get_info(series_id).await?.title;

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

        let response = self.send(request).await?; // Converts to SourceError::RequestFailed

        let body = response.text().await?;
        let response_json: serde_json::Value = serde_json::from_str(&body)?; // Converts to SourceError::JsonParseFailed

        Self::check_resp_errors(&response_json)?;

        let chapters = response_json["data"].as_array().ok_or_else(|| {
            warn!("No data found in response for series_id: {}", series_id);
            SourceError::SeriesNotFound {
                series_id: series_id.to_string(),
            }
        })?;

        if let Some(c) = chapters.first().cloned() {
            // Extract chapter number
            let chapter = c["attributes"]["chapter"]
                .as_str()
                .ok_or_else(|| SourceError::MissingField {
                    field: "chapter".to_string(),
                })?
                .to_string();

            // Extract and validate timestamp
            let publish_at = c["attributes"]["publishAt"]
                .as_str()
                .ok_or_else(|| SourceError::MissingField {
                    field: "publishAt".to_string(),
                })?
                .to_string();

            let published = DateTime::parse_from_rfc3339(&publish_at)
                .map(|dt| dt.with_timezone(&Utc))
                .map_err(|_| SourceError::InvalidTime { time: publish_at })?;

            info!(
                "Successfully fetched latest manga for series_id: {}",
                series_id
            );

            Ok(SourceResult::Series(SeriesLatestItem {
                id: series_id.to_string(),
                title,
                latest: chapter,
                url: self.get_url_from_id(series_id),
                published,
            }))
        } else {
            warn!("No chapters found in data for series_id: {}", series_id);
            Err(SourceError::EmptySeries {
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

    fn get_base(&self) -> &BaseSource<'_> {
        &self.base
    }
}

impl PartialEq for MangaDexSource<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.base.url.api_url == other.base.url.api_url
    }
}

impl Eq for MangaDexSource<'_> {}

impl Hash for MangaDexSource<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.base.url.api_url.hash(state);
    }
}

// #[cfg(test)]
// mod tests {
//     use super::*;
//     use httpmock::prelude::*;
//     use serde_json::json;
//     use tokio;
//
//     #[tokio::test]
//     async fn test_get_latest_returns_manga_struct() {
//         let server = MockServer::start();
//         let series_id = "789";
//         let chapter_id = "999";
//         let mock_response = json!({
//             "data": [
//                 {
//                     "id": chapter_id,
//                     "attributes": {
//                         "title": "Latest Chapter",
//                         "chapter": "42",
//                         "publishAt": "2025-07-14T02:35:03+00:00"
//                     }
//                 }
//             ]
//         });
//
//         let mock = server.mock(|when, then| {
//             when.method(GET)
//                 .path(format!("/manga/{}/feed", series_id))
//                 .query_param("order[createdAt]", "desc")
//                 .query_param("limit", "1");
//             then.status(200)
//                 .header("content-type", "application/json")
//                 .json_body(mock_response.clone());
//         });
//
//         let source = MangaDexSource::new_with_url(server.url(""));
//
//         let manga = source.get_latest(series_id).await.unwrap();
//         assert_eq!(manga.series_id, series_id);
//         assert_eq!(manga.series_type, "manga");
//         assert_eq!(manga.title, "Latest Chapter");
//         assert_eq!(manga.chapter, "42");
//         assert_eq!(manga.chapter_id, chapter_id);
//         assert_eq!(
//             manga.url,
//             format!("https://mangadex.org/chapter/{}", chapter_id)
//         );
//         mock.assert();
//     }
//
//     #[tokio::test]
//     async fn test_get_latest_handles_missing_fields() {
//         let server = MockServer::start();
//         let series_id = "missingfields";
//         let chapter_id = "abc";
//         let mock_response = json!({
//             "data": [
//                 {
//                     "id": chapter_id,
//                     "attributes": {}
//                 }
//             ]
//         });
//
//         let mock = server.mock(|when, then| {
//             when.method(GET)
//                 .path(format!("/manga/{}/feed", series_id))
//                 .query_param("order[createdAt]", "desc")
//                 .query_param("limit", "1");
//             then.status(200)
//                 .header("content-type", "application/json")
//                 .json_body(mock_response.clone());
//         });
//
//         let source = MangaDexSource::new_with_url(server.url(""));
//
//         let manga = source.get_latest(series_id).await.unwrap();
//         assert_eq!(manga.title, "Unknown");
//         assert_eq!(manga.chapter, "0");
//         assert_eq!(manga.chapter_id, chapter_id);
//         mock.assert();
//     }
// }
