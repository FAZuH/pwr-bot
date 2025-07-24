use reqwest::Client;
use super::anime::Anime;
use chrono::{DateTime, Utc};

use std::hash::{Hash, Hasher};

#[derive(Clone)]
pub struct AniListSource {
    client: Client,
    api_url: &'static str,
}

impl AniListSource {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
            api_url: "https://graphql.anilist.co",
        }
    }

    pub async fn get_latest(&self, series_id: &str) -> anyhow::Result<Option<Anime>> {
        let query = r#"
        query ($id: Int) {
            Media(id: $id, type: ANIME) {
                title { romaji }
                nextAiringEpisode { airingAt episode }
            }
        }
        "#;
        let json = serde_json::json!({ "query": query, "variables": { "id": series_id } });
        let response = self.client.post(self.api_url).json(&json).send().await?.json::<serde_json::Value>().await?;
        let episode = response["data"]["Media"]["nextAiringEpisode"].as_object();
        if episode.is_none() {
            return Ok(None);
        }
        let episode = episode.unwrap();
        Ok(Some(Anime {
            series_id: series_id.to_string(),
            series_type: "anime".to_string(),
            title: response["data"]["Media"]["title"]["romaji"].as_str().unwrap_or("Unknown").to_string(),
            episode: episode["episode"].as_i64().unwrap_or(0).to_string(),
            episode_id: format!("{}_{}", &series_id, episode["episode"].as_i64().unwrap_or(0)),
            url: format!("https://anilist.co/anime/{}", series_id),
            published: DateTime::from_timestamp(episode["airingAt"].as_i64().unwrap_or(0), 0)
                .unwrap_or(Utc::now()),
        }))
    }
}

impl PartialEq for AniListSource {
    fn eq(&self, other: &Self) -> bool {
        self.api_url == other.api_url
    }
}

impl Eq for AniListSource {}

impl Hash for AniListSource {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.api_url.hash(state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;
    use tokio;

    #[tokio::test]
    async fn test_get_latest_returns_anime_on_valid_response() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(POST);
            then.status(200)
                .json_body(serde_json::json!({
                    "data": {
                        "Media": {
                            "title": { "romaji": "Test Anime" },
                            "nextAiringEpisode": {
                                "airingAt": 1234567890,
                                "episode": 5
                            }
                        }
                    }
                }));
        });

        let source = AniListSource::new();

        let result = source.get_latest("123").await.unwrap();
        assert!(result.is_some());
        let anime = result.unwrap();
        assert_eq!(anime.series_id, "123");
        assert_eq!(anime.title, "Test Anime");
        assert_eq!(anime.episode, "5");
        assert_eq!(anime.episode_id, "123_5");
        assert_eq!(anime.url, "https://anilist.co/anime/123");
        mock.assert();
    }

    #[tokio::test]
    async fn test_get_latest_returns_none_when_no_next_airing_episode() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(POST);
            then.status(200)
                .json_body(serde_json::json!({
                    "data": {
                        "Media": {
                            "title": { "romaji": "Test Anime" },
                            "nextAiringEpisode": null
                        }
                    }
                }));
        });

        let source = AniListSource::new();

        let result = source.get_latest("456").await.unwrap();
        assert!(result.is_none());
        mock.assert();
    }

    #[tokio::test]
    async fn test_get_latest_handles_missing_title() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(POST);
            then.status(200)
                .json_body(serde_json::json!({
                    "data": {
                        "Media": {
                            "title": {},
                            "nextAiringEpisode": {
                                "airingAt": 1234567890,
                                "episode": 7
                            }
                        }
                    }
                }));
        });

        let source = AniListSource::new();

        let result = source.get_latest("789").await.unwrap();
        assert!(result.is_some());
        let anime = result.unwrap();
        assert_eq!(anime.title, "Unknown");
        assert_eq!(anime.episode, "7");
        mock.assert();
    }

    #[tokio::test]
    async fn test_get_latest_handles_invalid_json() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(POST);
            then.status(200)
                .body("not a json");
        });

        let source = AniListSource::new();

        let result = source.get_latest("999").await;
        assert!(result.is_err());
        mock.assert();
    }
}

