//! Internal service for bot metadata and maintenance operations.

use std::sync::Arc;

use crate::entity::BotMetaEntity;
use crate::entity::BotMetaKey;
use crate::entity::FeedEntity;
use crate::entity::FeedItemEntity;
use crate::entity::FeedSubscriptionEntity;
use crate::entity::SubscriberEntity;
use crate::repo::error::DatabaseError;
use crate::repo::traits::*;
use crate::service::traits::InternalOps;

#[async_trait::async_trait]
impl InternalOps for InternalService {
    async fn get_meta(&self, key: BotMetaKey) -> Result<Option<String>, DatabaseError> {
        self.get_meta(key).await
    }

    async fn set_meta(&self, key: BotMetaKey, value: String) -> Result<(), DatabaseError> {
        self.set_meta(key, value).await
    }

    async fn dump_database(&self) -> anyhow::Result<DatabaseDump> {
        self.dump_database().await
    }
}

/// Internal service for metadata and maintenance operations.
pub struct InternalService {
    feed: Arc<dyn FeedRepository + Send + Sync>,
    feed_item: Arc<dyn FeedItemRepository + Send + Sync>,
    subscriber: Arc<dyn SubscriberRepository + Send + Sync>,
    feed_subscription: Arc<dyn FeedSubscriptionRepository + Send + Sync>,
    bot_meta: Arc<dyn BotMetaRepository + Send + Sync>,
}

impl InternalService {
    /// Creates a new internal service.
    pub fn new(
        feed: Arc<dyn FeedRepository + Send + Sync>,
        feed_item: Arc<dyn FeedItemRepository + Send + Sync>,
        subscriber: Arc<dyn SubscriberRepository + Send + Sync>,
        feed_subscription: Arc<dyn FeedSubscriptionRepository + Send + Sync>,
        bot_meta: Arc<dyn BotMetaRepository + Send + Sync>,
    ) -> Self {
        Self {
            feed,
            feed_item,
            subscriber,
            feed_subscription,
            bot_meta,
        }
    }

    /// Get a metadata value by key.
    pub async fn get_meta(&self, key: BotMetaKey) -> Result<Option<String>, DatabaseError> {
        let result: Option<BotMetaEntity> = self.bot_meta.select(&key.into()).await?;
        Ok(result.map(|m| m.value))
    }

    /// Set a metadata value by key (upsert).
    pub async fn set_meta(
        &self,
        key: BotMetaKey,
        value: impl Into<String>,
    ) -> Result<(), DatabaseError> {
        let model = BotMetaEntity {
            key: key.into(),
            value: value.into(),
        };
        self.bot_meta.replace(&model).await?;
        Ok(())
    }

    /// Dumps all database tables for inspection.
    pub async fn dump_database(&self) -> anyhow::Result<DatabaseDump> {
        let feeds = self.feed.select_all().await?;
        let feed_items = self.feed_item.select_all().await?;
        let subscribers = self.subscriber.select_all().await?;
        let subscriptions = self.feed_subscription.select_all().await?;

        Ok(DatabaseDump {
            feeds,
            feed_items,
            subscribers,
            subscriptions,
        })
    }
}

/// Container for a full database dump.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DatabaseDump {
    pub feeds: Vec<FeedEntity>,
    pub feed_items: Vec<FeedItemEntity>,
    pub subscribers: Vec<SubscriberEntity>,
    pub subscriptions: Vec<FeedSubscriptionEntity>,
}
