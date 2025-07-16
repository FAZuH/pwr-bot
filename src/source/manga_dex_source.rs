use reqwest::header::{HeaderMap, HeaderValue, USER_AGENT};
use reqwest::Client;
use super::manga::Manga;
use std::hash::{Hash, Hasher};


#[derive(Clone)]
pub struct MangaDexSource {
    client: Client,
    api_url: String,
}

impl MangaDexSource {
    pub fn new(api_url: String) -> Self {
        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static("pwr-bot/0.1"));

        let client = Client::builder()
            .default_headers(headers)
            .build()
            .expect("Failed to create client");

        Self {
            client: client,
            api_url: api_url.clone(),
        }
    }

    pub async fn get_latest(&self, series_id: &str) -> anyhow::Result<Option<Manga>> {
        let chapter = self.get_latest_chapter(series_id).await?;
        let ret = chapter.map(|c| Manga {
            series_id: series_id.to_string(),
            series_type: "manga".to_string(),
            title: c["attributes"]["title"].as_str().unwrap_or("Unknown").to_string(),
            chapter: c["attributes"]["chapter"].as_str().unwrap_or("0").to_string(),
            chapter_id: c["id"].as_str().unwrap_or("").to_string(),
            url: format!("https://mangadex.org/chapter/{}", c["id"].as_str().unwrap_or("")),
        });
        Ok(ret)
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

impl PartialEq for MangaDexSource {
    fn eq(&self, other: &Self) -> bool {
        self.api_url == other.api_url
    }
}

impl Eq for MangaDexSource {}

impl Hash for MangaDexSource {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.api_url.hash(state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;
    use serde_json::json;
    use tokio;

    #[tokio::test]
    async fn test_get_latest_chapter_returns_chapter() {
        let server = MockServer::start();
        let series_id = "123";
        let chapter_id = "456";
        let mock_response = json!({
            "data": [
                {
                    "id": chapter_id,
                    "attributes": {
                        "title": "Test Chapter",
                        "chapter": "10"
                    }
                }
            ]
        });

        let _mock = server.mock(|when, then| {
            when.method(GET)
                .path(format!("/manga/{}/feed", series_id))
                .query_param("order[createdAt]", "desc")
                .query_param("limit", "1");
            then.status(200)
                .header("content-type", "application/json")
                .json_body(mock_response.clone());
        });

        let source = MangaDexSource::new(server.base_url());

        let result = source.get_latest_chapter(series_id).await.unwrap();
        assert!(result.is_some());
        let chapter = result.unwrap();
        assert_eq!(chapter["id"], chapter_id);
        assert_eq!(chapter["attributes"]["title"], "Test Chapter");
        assert_eq!(chapter["attributes"]["chapter"], "10");
    }

    #[tokio::test]
    async fn test_get_latest_chapter_returns_none_when_no_chapters() {
        let server = MockServer::start();
        let series_id = "empty";
        let mock_response = json!({ "data": [] });

        let _mock = server.mock(|when, then| {
            when.method(GET)
                .path(format!("/manga/{}/feed", series_id))
                .query_param("order[createdAt]", "desc")
                .query_param("limit", "1");
            then.status(200)
                .header("content-type", "application/json")
                .json_body(mock_response.clone());
        });

        let source = MangaDexSource::new(server.base_url());

        let result = source.get_latest_chapter(series_id).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_get_latest_returns_manga_struct() {
        let server = MockServer::start();
        let series_id = "789";
        let chapter_id = "999";
        let mock_response = json!({
            "data": [
                {
                    "id": chapter_id,
                    "attributes": {
                        "title": "Latest Chapter",
                        "chapter": "42"
                    }
                }
            ]
        });

        let _mock = server.mock(|when, then| {
            when.method(GET)
                .path(format!("/manga/{}/feed", series_id))
                .query_param("order[createdAt]", "desc")
                .query_param("limit", "1");
            then.status(200)
                .header("content-type", "application/json")
                .json_body(mock_response.clone());
        });

        let source = MangaDexSource::new(server.base_url());

        let result = source.get_latest(series_id).await.unwrap();
        assert!(result.is_some());
        let manga = result.unwrap();
        assert_eq!(manga.series_id, series_id);
        assert_eq!(manga.series_type, "manga");
        assert_eq!(manga.title, "Latest Chapter");
        assert_eq!(manga.chapter, "42");
        assert_eq!(manga.chapter_id, chapter_id);
        assert_eq!(manga.url, format!("https://mangadex.org/chapter/{}", chapter_id));
    }

    #[tokio::test]
    async fn test_get_latest_returns_none_when_no_chapters() {
        let server = MockServer::start();
        let series_id = "nochap";
        let mock_response = json!({ "data": [] });

        let _mock = server.mock(|when, then| {
            when.method(GET)
                .path(format!("/manga/{}/feed", series_id))
                .query_param("order[createdAt]", "desc")
                .query_param("limit", "1");
            then.status(200)
                .header("content-type", "application/json")
                .json_body(mock_response.clone());
        });

        let source = MangaDexSource::new(server.base_url());

        let result = source.get_latest(series_id).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_get_latest_handles_missing_fields() {
        let server = MockServer::start();
        let series_id = "missingfields";
        let chapter_id = "abc";
        let mock_response = json!({
            "data": [
                {
                    "id": chapter_id,
                    "attributes": {}
                }
            ]
        });

        let _mock = server.mock(|when, then| {
            when.method(GET)
                .path(format!("/manga/{}/feed", series_id))
                .query_param("order[createdAt]", "desc")
                .query_param("limit", "1");
            then.status(200)
                .header("content-type", "application/json")
                .json_body(mock_response.clone());
        });

        let source = MangaDexSource::new(server.base_url());

        let result = source.get_latest(series_id).await.unwrap();
        assert!(result.is_some());
        let manga = result.unwrap();
        assert_eq!(manga.title, "Unknown");
        assert_eq!(manga.chapter, "0");
        assert_eq!(manga.chapter_id, chapter_id);
    }
}
