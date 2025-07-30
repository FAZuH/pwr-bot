use super::manga::Manga;
use chrono::{DateTime, Utc};
use log::{info, warn};
use reqwest::Client;
use reqwest::header::{HeaderMap, HeaderValue, USER_AGENT};
use std::hash::{Hash, Hasher};

#[derive(Clone)]
pub struct MangaDexSource {
    pub base: super::base_source::BaseSource,
}

impl MangaDexSource {
    pub fn new() -> Self {
        Self::new_with_url("https://api.mangadex.org".to_string())
    }

    pub fn new_with_url(api_url: String) -> Self {
        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static("pwr-bot/0.1"));

        let client = Client::builder()
            .default_headers(headers)
            .build()
            .expect("Failed to create client");

        Self {
            base: super::base_source::BaseSource::new("mangadex.org".to_string(), api_url, client),
        }
    }

    pub async fn get_latest(&self, series_id: &str) -> anyhow::Result<Manga> {
        info!("Fetching latest manga for series_id: {}", series_id);
        let url = format!(
            "{}/manga/{}/feed?order[createdAt]=desc&limit=1&translatedLanguage[]=en&translatedLanguage[]=id",
            self.base.api_url, series_id
        );
        let response = self.base.client.get(&url).send().await?;
        let body = response.text().await?;
        let response_json: serde_json::Value = serde_json::from_str(&body)?;
        let chapters = response_json["data"].as_array().ok_or_else(|| {
            warn!(
                "No chapters array found in response for series_id: {}",
                series_id
            );
            anyhow::anyhow!("No chapters found")
        })?;
        if let Some(c) = chapters.get(0).cloned() {
            info!(
                "Successfully fetched latest manga for series_id: {}",
                series_id
            );
            Ok(Manga {
                series_id: series_id.to_string(),
                series_type: "manga".to_string(),
                title: c["attributes"]["title"]
                    .as_str()
                    .unwrap_or("Unknown")
                    .to_string(),
                chapter: c["attributes"]["chapter"]
                    .as_str()
                    .unwrap_or("0")
                    .to_string(),
                chapter_id: c["id"].as_str().unwrap_or("").to_string(),
                url: format!(
                    "https://mangadex.org/chapter/{}",
                    c["id"].as_str().unwrap_or("")
                ),
                published: DateTime::parse_from_rfc3339(
                    c["attributes"]["publishAt"]
                        .as_str()
                        .unwrap_or("1970-01-01T00:00:00Z"),
                )
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or(Utc::now()),
            })
        } else {
            warn!("No chapters found in data for series_id: {}", series_id);
            return Err(anyhow::anyhow!("No chapters found"));
        }
    }

    pub async fn get_title(&self, series_id: &str) -> anyhow::Result<String> {
        let url = format!("{}/manga/{}", self.base.api_url, series_id);
        let body = self.base.client.get(&url).send().await?.text().await?;
        let response_json: serde_json::Value = serde_json::from_str(&body)?;
        let ret = response_json["data"]["attributes"]["title"]["en"]
            .as_str()
            .unwrap_or("Title not found")
            .to_string();
        Ok(ret)
    }

    pub fn get_id_from_url(&self, url: &String) -> Result<String, super::error::UrlParseError> {
        self.base.get_nth_path_from_url(url, 2)
    }
}

impl PartialEq for MangaDexSource {
    fn eq(&self, other: &Self) -> bool {
        self.base.api_url == other.base.api_url
    }
}

impl Eq for MangaDexSource {}

impl Hash for MangaDexSource {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.base.api_url.hash(state);
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
