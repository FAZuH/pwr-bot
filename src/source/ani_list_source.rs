use async_trait::async_trait;
use reqwest::Client;
use crate::config::Config;
use crate::event::new_chapter_event::NewChapterEvent;
use crate::source::source::UpdateSource;

pub struct AniListSource {
    client: Client,
    api_url: String,
}

impl AniListSource {
    pub fn new(config: &Config) -> Self {
        Self {
            client: Client::new(),
            api_url: config.anilist_api_url.clone(),
        }
    }
}

#[async_trait]
impl UpdateSource for AniListSource {
    async fn check_update(&self, series_id: &str) -> anyhow::Result<Option<NewChapterEvent>> {
        let query = r#"
        query ($id: Int) {
            Media(id: $id, type: ANIME) {
                title { romaji }
                nextAiringEpisode { airingAt episode }
            }
        }
        "#;
        let json = serde_json::json!({ "query": query, "variables": { "id": series_id } });
        let response = self.client.post(&self.api_url).json(&json).send().await?.json::<serde_json::Value>().await?;
        let episode = response["data"]["Media"]["nextAiringEpisode"].as_object();
        if episode.is_none() {
            return Ok(None);
        }
        let episode = episode.unwrap();
        Ok(Some(NewChapterEvent {
            series_id: series_id.to_string(),
            series_type: "anime".to_string(),
            chapter_title: response["data"]["Media"]["title"]["romaji"].as_str().unwrap_or("Unknown").to_string(),
            content: episode["episode"].as_i64().unwrap_or(0).to_string(),
            content_id: format!("{}_{}", series_id, episode["episode"].as_i64().unwrap_or(0)),
            url: format!("https://anilist.co/anime/{}", series_id),
        }))
    }
}
