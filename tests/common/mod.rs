use std::path::PathBuf;
use std::sync::Arc;
use std::sync::RwLock;

use async_trait::async_trait;
use pwr_bot::database::Database;
use pwr_bot::feed::BaseFeed;
use pwr_bot::feed::Feed;
use pwr_bot::feed::FeedInfo;
use pwr_bot::feed::FeedItem;
use pwr_bot::feed::FeedSource;
use pwr_bot::feed::error::FeedError;
use pwr_bot::feed::error::UrlParseError;
use uuid::Uuid;

pub async fn setup_db() -> (Arc<Database>, PathBuf) {
    let uuid = Uuid::new_v4();
    let db_path = std::env::temp_dir().join(format!("pwr-bot-test-{}.db", uuid));
    let db_url = format!("sqlite://{}", db_path.to_str().unwrap());

    let db = Database::new(&db_url, db_path.to_str().unwrap())
        .await
        .expect("Failed to create database");

    db.run_migrations().await.expect("Failed to run migrations");

    (Arc::new(db), db_path)
}

pub async fn teardown_db(db_path: PathBuf) {
    if db_path.exists() {
        let _ = std::fs::remove_file(db_path);
    }
}

// MOCK FEED

#[derive(Clone)]
#[allow(dead_code)]
pub struct MockFeed {
    pub base: BaseFeed,
    pub state: Arc<RwLock<MockFeedState>>,
}

#[derive(Default, Clone)]
#[allow(dead_code)]
pub struct MockFeedState {
    pub feed_source: FeedSource,
    pub feed_item: Option<FeedItem>,
}

#[allow(dead_code)]
impl MockFeed {
    pub fn new(domain: &str) -> Self {
        let info = FeedInfo {
            name: "MockFeed".to_string(),
            feed_item_name: "Chapter".to_string(),
            api_hostname: format!("api.{}", domain),
            api_domain: domain.to_string(),
            api_url: format!("https://api.{}", domain),
            copyright_notice: "Mock".to_string(),
            logo_url: "".to_string(),
            tags: "series".to_string(),
        };
        let client = reqwest::Client::new();

        Self {
            base: BaseFeed::new(info, client),
            state: Arc::new(RwLock::new(MockFeedState::default())),
        }
    }

    pub fn set_latest(&self, latest: Option<FeedItem>) {
        self.state.write().unwrap().feed_item = latest;
    }

    pub fn set_info(&self, item: FeedSource) {
        self.state.write().unwrap().feed_source = item;
    }
}

#[async_trait]
impl Feed for MockFeed {
    async fn fetch_latest(&self, id: &str) -> Result<FeedItem, FeedError> {
        if let Some(feed_item) = &self.state.read().unwrap().feed_item {
            Ok(feed_item.clone())
        } else {
            Err(FeedError::ItemNotFound {
                source_id: id.to_string(),
            })
        }
    }

    async fn fetch_source(&self, _id: &str) -> Result<FeedSource, FeedError> {
        Ok(self.state.read().unwrap().feed_source.clone())
    }

    fn get_id_from_url<'a>(&self, url: &'a str) -> Result<&'a str, UrlParseError> {
        self.base.get_nth_path_from_url(url, 1)
    }

    fn get_url_from_id(&self, id: &str) -> String {
        format!("https://{}/title/{}", self.base.info.api_domain, id)
    }

    fn get_base(&self) -> &BaseFeed {
        &self.base
    }
}
