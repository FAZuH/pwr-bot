use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue, USER_AGENT};
use reqwest::Client;
use crate::config::Config;
use crate::event::new_chapter_event::NewChapterEvent;
use crate::source::source::UpdateSource;

pub struct MangaDexSource {
    client: Client,
    api_url: String,
}

impl MangaDexSource {
    pub fn new(config: &Config) -> Self {
        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static("pwr-bot/0.1"));

        let client = Client::builder()
            .default_headers(headers)
            .build()
            .expect("Failed to create client");

        Self {
            client,
            api_url: config.mangadex_api_url.clone(),
        }
    }

    async fn get_latest_chapter(&self, series_id: &str) -> anyhow::Result<Option<serde_json::Value>> {
        let url = format!("{}/manga/{}/feed?order[createdAt]=desc&limit=1", self.api_url, series_id);
        let response = self.client.get(&url).send().await?;
        let body = response.text().await?;
        let response_json: serde_json::Value = serde_json::from_str(&body)?;
        let chapters = response_json["data"].as_array().ok_or_else(|| anyhow::anyhow!("No chapters found"))?;
        if chapters.is_empty() {
            return Ok(None);
        }
        let chapter = chapters.get(0).cloned();
        Ok(chapter)
    }
}

#[async_trait]
impl UpdateSource for MangaDexSource {
    async fn check_update(&self, series_id: &str) -> anyhow::Result<Option<NewChapterEvent>> {
        let chapter = self.get_latest_chapter(series_id).await?.expect("Error fetching latest chapter from MangaDex API");
        Ok(Some(NewChapterEvent {
            series_id: series_id.to_string(),
            series_type: "manga".to_string(),
            chapter_title: chapter["attributes"]["title"].as_str().unwrap_or("Unknown").to_string(),
            content: chapter["attributes"]["chapter"].as_str().unwrap_or("0").to_string(),
            content_id: chapter["id"].as_str().unwrap_or("").to_string(),
            url: format!("https://mangadex.org/chapter/{}", chapter["id"].as_str().unwrap_or("")),
        }))
    }
}
