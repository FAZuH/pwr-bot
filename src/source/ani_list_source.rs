use super::anime::Anime;
use chrono::DateTime;
use log::{info, warn};
use reqwest::Client;
use super::error::SourceError;

use std::hash::{Hash, Hasher};

#[derive(Clone)]
pub struct AniListSource {
    pub base: super::base_source::BaseSource,
}

impl AniListSource {
    pub fn new() -> Self {
        Self::new_with_url("https://graphql.anilist.co".to_string())
    }

    pub fn new_with_url(api_url: String) -> Self {
        Self {
            base: super::base_source::BaseSource::new("anilist.co".to_string(), api_url, Client::new()),
        }
    }

    pub async fn get_latest(&self, series_id: &str) -> Result<Anime, SourceError> {
        info!("Fetching latest anime for series_id: {}", series_id);
        
        // Validate series_id format (should be numeric for AniList)
        let series_id_num = series_id.parse::<i32>()
            .map_err(|_| SourceError::InvalidSeriesId { 
                series_id: series_id.to_string() 
            })?;
        
        let query = r#"
        query ($id: Int) {
            Media(id: $id, type: ANIME) {
                title { romaji }
                nextAiringEpisode { airingAt episode }
            }
        }
        "#;
        
        let json = serde_json::json!({ 
            "query": query, 
            "variables": { "id": series_id_num } 
        });
        
        let response = self.base
            .client
            .post(&self.base.api_url)
            .json(&json)
            .send()
            .await?; // Automatically converts to SourceError::RequestFailed
        
        let body = response.json::<serde_json::Value>().await?; // Automatically converts to SourceError::JsonParseFailed
        
        // Check for GraphQL errors
        if let Some(errors) = body.get("errors") {
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
        
        // Check if Media exists and is not null
        let media = body["data"]["Media"].as_object()
            .ok_or_else(|| SourceError::SeriesNotFound { 
                series_id: series_id.to_string() 
            })?;
        
        // Check for next airing episode
        let next_episode = media.get("nextAiringEpisode");
        if next_episode.is_none() || next_episode.unwrap().is_null() {
            warn!("No next airing episode found for series_id: {}", series_id);
            return Err(SourceError::FinishedSeries { 
                series_id: series_id.to_string() 
            });
        }
        
        let episode_obj = next_episode.unwrap().as_object()
            .ok_or_else(|| SourceError::MissingField { 
                field: "nextAiringEpisode".to_string() 
            })?;
        
        // Extract episode number
        let episode = episode_obj.get("episode")
            .and_then(|e| e.as_i64())
            .ok_or_else(|| SourceError::MissingField { 
                field: "episode".to_string() 
            })?.to_string();
        
        // Extract airing timestamp
        let airing_timestamp = episode_obj.get("airingAt")
            .and_then(|a| a.as_i64())
            .ok_or_else(|| SourceError::MissingField { 
                field: "airingAt".to_string() 
            })?;
        
        // Validate timestamp
        let published = DateTime::from_timestamp(airing_timestamp, 0)
            .ok_or_else(|| SourceError::InvalidTimestamp { 
                timestamp: airing_timestamp 
            })?;
        
        // Extract title
        let title = body["data"]["Media"]["title"]["romaji"]
            .as_str()
            .unwrap_or("Unknown")
            .to_string();
        
        info!("Successfully fetched anime for series_id: {}", series_id);
        
        Ok(Anime {
            series_id: series_id.to_string(),
            series_type: "anime".to_string(),
            title,
            episode,
            url: format!("https://anilist.co/anime/{}", series_id),
            published,
        })
    }

    pub fn get_id_from_url(&self, url: &String) -> Result<String, super::error::UrlParseError> {
        self.base.get_nth_path_from_url(url, 1)
    }
}

impl PartialEq for AniListSource {
    fn eq(&self, other: &Self) -> bool {
        self.base.api_url == other.base.api_url
    }
}

impl Eq for AniListSource {}

impl Hash for AniListSource {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.base.api_url.hash(state);
    }
}

// #[cfg(test)]
// mod tests {
//     use super::*;
//     use httpmock::prelude::*;
//     use tokio;
//
//     #[tokio::test]
//     async fn test_get_latest_returns_anime_on_valid_response() {
//         let server = MockServer::start();
//         let mock = server.mock(|when, then| {
//             when.method(POST);
//             then.status(200).json_body(serde_json::json!({
//                 "data": {
//                     "Media": {
//                         "title": { "romaji": "Test Anime" },
//                         "nextAiringEpisode": {
//                             "airingAt": 1234567890,
//                             "episode": 5
//                         }
//                     }
//                 }
//             }));
//         });
//
//         let source = AniListSource::new_with_url(server.url(""));
//
//         let anime = source.get_latest("123").await.unwrap();
//         assert_eq!(anime.series_id, "123");
//         assert_eq!(anime.title, "Test Anime");
//         assert_eq!(anime.episode, "5");
//         assert_eq!(anime.url, "https://anilist.co/anime/123");
//         mock.assert();
//     }
//
//     #[tokio::test]
//     async fn test_get_latest_returns_error_when_no_next_airing_episode() {
//         // Fixed test name and logic
//         let server = MockServer::start();
//         let mock = server.mock(|when, then| {
//             when.method(POST);
//             then.status(200).json_body(serde_json::json!({
//                 "data": {
//                     "Media": {
//                         "title": { "romaji": "Test Anime" },
//                         "nextAiringEpisode": null
//                     }
//                 }
//             }));
//         });
//
//         let source = AniListSource::new_with_url(server.url(""));
//
//         let result = source.get_latest("456").await;
//         assert!(result.is_err()); // Should return error when nextAiringEpisode is null
//         mock.assert();
//     }
//
//     #[tokio::test]
//     async fn test_get_latest_handles_missing_title() {
//         let server = MockServer::start();
//         let mock = server.mock(|when, then| {
//             when.method(POST);
//             then.status(200).json_body(serde_json::json!({
//                 "data": {
//                     "Media": {
//                         "title": {},
//                         "nextAiringEpisode": {
//                             "airingAt": 1234567890,
//                             "episode": 7
//                         }
//                     }
//                 }
//             }));
//         });
//
//         let source = AniListSource::new_with_url(server.url(""));
//
//         let anime = source.get_latest("789").await.unwrap();
//         assert_eq!(anime.title, "Unknown");
//         assert_eq!(anime.episode, "7");
//         mock.assert();
//     }
//
//     #[tokio::test]
//     async fn test_get_latest_handles_invalid_json() {
//         let server = MockServer::start();
//         let mock = server.mock(|when, then| {
//             when.method(POST);
//             then.status(200).body("not a json");
//         });
//
//         let source = AniListSource::new_with_url(server.url(""));
//
//         let result = source.get_latest("999").await;
//         assert!(result.is_err());
//         mock.assert();
//     }
// }
