//! Common test utilities and mock implementations.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::RwLock;

use async_trait::async_trait;
use pwr_bot::feed::BasePlatform;
use pwr_bot::feed::FeedItem;
use pwr_bot::feed::FeedSource;
use pwr_bot::feed::Platform;
use pwr_bot::feed::PlatformInfo;
use pwr_bot::feed::error::FeedError;
use pwr_bot::repository::Repository;
use uuid::Uuid;

/// Sets up a temporary test database.
pub async fn setup_db() -> (Arc<Repository>, PathBuf) {
    let uuid = Uuid::new_v4();
    let db_path = std::env::temp_dir().join(format!("pwr-bot-test-{}.db", uuid));
    let db_url = format!("sqlite://{}", db_path.to_str().unwrap());

    let db = Repository::new(&db_url, db_path.to_str().unwrap())
        .await
        .expect("Failed to create database");

    db.run_migrations().await.expect("Failed to run migrations");

    (Arc::new(db), db_path)
}

/// Cleans up the test database file.
pub async fn teardown_db(db_path: PathBuf) {
    if db_path.exists() {
        let _ = std::fs::remove_file(db_path);
    }
}

// MOCK FEED

/// Mock feed platform for testing.
#[derive(Clone)]
#[allow(dead_code)]
pub struct MockFeed {
    pub base: BasePlatform,
    pub state: Arc<RwLock<MockFeedState>>,
}

/// State for the mock feed.
#[derive(Default, Clone)]
#[allow(dead_code)]
pub struct MockFeedState {
    pub feed_source: FeedSource,
    pub feed_item: Option<FeedItem>,
}

#[allow(dead_code)]
impl MockFeed {
    /// Creates a new mock feed with the given domain.
    pub fn new(domain: &str) -> Self {
        let info = PlatformInfo {
            name: "MockFeed".to_string(),
            feed_item_name: "Chapter".to_string(),
            api_hostname: format!("api.{}", domain),
            api_domain: domain.to_string(),
            api_url: format!("https://api.{}", domain),
            copyright_notice: "Mock".to_string(),
            logo_url: "".to_string(),
            tags: "series".to_string(),
        };
        Self {
            base: BasePlatform::new(info),
            state: Arc::new(RwLock::new(MockFeedState::default())),
        }
    }

    /// Sets the latest feed item.
    pub fn set_latest(&self, latest: Option<FeedItem>) {
        self.state.write().unwrap().feed_item = latest;
    }

    /// Sets the feed source information.
    pub fn set_info(&self, item: FeedSource) {
        self.state.write().unwrap().feed_source = item;
    }
}

#[async_trait]
impl Platform for MockFeed {
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

    fn get_id_from_source_url<'a>(&self, url: &'a str) -> Result<&'a str, FeedError> {
        Ok(self.base.get_nth_path_from_url(url, 1)?)
    }

    fn get_source_url_from_id(&self, id: &str) -> String {
        format!("https://{}/title/{}", self.base.info.api_domain, id)
    }

    fn get_base(&self) -> &BasePlatform {
        &self.base
    }
}
