//! Common test utilities and mock implementations.

use std::sync::Arc;
use std::sync::RwLock;

use async_trait::async_trait;
use pwr_bot::feed::BasePlatform;
use pwr_bot::feed::FeedItem;
use pwr_bot::feed::FeedSource;
use pwr_bot::feed::Platform;
use pwr_bot::feed::PlatformInfo;
use pwr_bot::feed::error::FeedError;
use pwr_bot::repo::Repository;

/// Sets up a test database connection to PostgreSQL.
pub async fn setup_db() -> Arc<Repository> {
    let db_url = std::env::var("DB_URL")
        .unwrap_or("postgres://pwr_bot:pwr_bot@localhost:5432/pwr_bot".to_string());

    let db = Repository::new(&db_url)
        .await
        .expect("Failed to connect to database");

    db.delete_all_tables()
        .await
        .expect("Failed to clean database");

    db.run_migrations().await.expect("Failed to run migrations");

    Arc::new(db)
}

/// Cleans up the test database by deleting all data.
pub async fn teardown_db(db: &Repository) {
    db.delete_all_tables()
        .await
        .expect("Failed to clean database");
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
