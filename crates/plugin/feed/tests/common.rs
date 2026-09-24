use std::sync::Arc;
use std::sync::RwLock;

use async_trait::async_trait;
use diesel_async::RunQueryDsl;
use feed::BasePlatform;
use feed::FeedItem;
use feed::FeedSource;
use feed::Platform;
use feed::PlatformInfo;
use feed::feed::error::FeedError;
use feed::repo::Repository;

#[path = "support/db.rs"]
pub mod db;

pub async fn setup_db() -> Repository {
    let db_url = db::db_url().await;
    let repository = Repository::connect(&db_url)
        .await
        .expect("connect feed test database");
    let mut connection = repository.pool().get().await.expect("connect core table");
    diesel::sql_query(concat!(
        "CREATE TABLE IF NOT EXISTS server_settings ",
        "(guild_id BIGINT PRIMARY KEY, settings JSONB NOT NULL)"
    ))
    .execute(&mut connection)
    .await
    .expect("provision the host legacy settings table");
    diesel::sql_query("DELETE FROM server_settings")
        .execute(&mut connection)
        .await
        .expect("clear the host legacy settings table");
    drop(connection);
    repository
        .migrate()
        .await
        .expect("run feed migrations on the test database");
    repository.delete_all().await.expect("clean feed database");
    repository
}

pub async fn teardown_db(repository: &Repository) {
    repository.delete_all().await.expect("clean feed database");
}

#[derive(Clone)]
pub struct MockFeed {
    pub base: BasePlatform,
    pub state: Arc<RwLock<MockFeedState>>,
}

#[derive(Default, Clone)]
pub struct MockFeedState {
    pub feed_source: FeedSource,
    pub feed_item: Option<FeedItem>,
}

impl MockFeed {
    pub fn new(domain: &str) -> Self {
        let info = PlatformInfo {
            name: "MockFeed".to_string(),
            feed_item_name: "Chapter".to_string(),
            api_hostname: format!("api.{domain}"),
            api_domain: domain.to_string(),
            api_url: format!("https://api.{domain}"),
            copyright_notice: "Mock".to_string(),
            logo_url: String::new(),
            tags: "series".to_string(),
        };
        Self {
            base: BasePlatform::new(info),
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
