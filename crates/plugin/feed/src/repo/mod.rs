pub mod error;
pub mod feed_settings;
pub mod postgres;
pub mod schema;
pub mod traits;

use crate::repo::error::DatabaseError;
use crate::repo::feed_settings::PgFeedSettingsRepo;
use crate::repo::postgres::PgFeedItemRepo;
use crate::repo::postgres::PgFeedRepo;
use crate::repo::postgres::PgFeedSubscriptionRepo;
use crate::repo::postgres::PgSubscriberRepo;
use crate::repo::traits::FeedItemRepository;
use crate::repo::traits::FeedRepository;
use crate::repo::traits::FeedSubscriptionRepository;
use crate::repo::traits::Repos;
use crate::repo::traits::SubscriberRepository;
use crate::repo::traits::TableBase;
use crate::storage::DbPool;
use crate::storage::Store;

#[derive(Clone)]
pub struct Repository {
    pub feed: PgFeedRepo,
    pub feed_item: PgFeedItemRepo,
    pub subscriber: PgSubscriberRepo,
    pub feed_subscription: PgFeedSubscriptionRepo,
    pub feed_settings: PgFeedSettingsRepo,
    store: Store,
}

impl Repository {
    pub async fn connect(db_url: impl Into<String>) -> anyhow::Result<Self> {
        let store = Store::connect(db_url).await?;
        let pool = store.pool().clone();
        Ok(Self {
            feed: PgFeedRepo::new(pool.clone()),
            feed_item: PgFeedItemRepo::new(pool.clone()),
            subscriber: PgSubscriberRepo::new(pool.clone()),
            feed_subscription: PgFeedSubscriptionRepo::new(pool.clone()),
            feed_settings: PgFeedSettingsRepo::new(pool),
            store,
        })
    }

    pub async fn migrate(&self) -> anyhow::Result<Vec<String>> {
        self.store.migrate().await
    }

    pub fn pool(&self) -> &DbPool {
        self.store.pool()
    }

    pub async fn delete_all(&self) -> Result<(), DatabaseError> {
        self.feed_subscription.delete_all().await?;
        self.subscriber.delete_all().await?;
        self.feed_item.delete_all().await?;
        self.feed.delete_all().await?;
        self.feed_settings.delete_all().await?;
        Ok(())
    }
}

impl Repos for Repository {
    fn feed(&self) -> Box<dyn FeedRepository + Send + Sync> {
        Box::new(self.feed.clone())
    }

    fn feed_item(&self) -> Box<dyn FeedItemRepository + Send + Sync> {
        Box::new(self.feed_item.clone())
    }

    fn subscriber(&self) -> Box<dyn SubscriberRepository + Send + Sync> {
        Box::new(self.subscriber.clone())
    }

    fn feed_subscription(&self) -> Box<dyn FeedSubscriptionRepository + Send + Sync> {
        Box::new(self.feed_subscription.clone())
    }
}
