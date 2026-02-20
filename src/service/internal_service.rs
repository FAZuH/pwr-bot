//! Internal service for bot metadata and maintenance operations.

use std::sync::Arc;

use crate::model::BotMetaModel;
use crate::model::FeedItemModel;
use crate::model::FeedModel;
use crate::model::FeedSubscriptionModel;
use crate::model::SubscriberModel;
use crate::repository::Repository;
use crate::repository::error::DatabaseError;
use crate::repository::table::Table;

/// Internal service for metadata and maintenance operations.
pub struct InternalService {
    db: Arc<Repository>,
}

impl InternalService {
    /// Creates a new internal service.
    pub fn new(db: Arc<Repository>) -> Self {
        Self { db }
    }

    /// Get a metadata value by key.
    pub async fn get_meta(&self, key: impl Into<String>) -> Result<Option<String>, DatabaseError> {
        let result = self.db.bot_meta.select(&key.into()).await?;
        Ok(result.map(|m| m.value))
    }

    /// Set a metadata value by key (upsert).
    pub async fn set_meta(
        &self,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<(), DatabaseError> {
        let model = BotMetaModel {
            key: key.into(),
            value: value.into(),
        };
        self.db.bot_meta.replace(&model).await?;
        Ok(())
    }

    /// Dumps all database tables for inspection.
    pub async fn dump_database(&self) -> anyhow::Result<DatabaseDump> {
        let feeds = self.db.feed.select_all().await?;
        let feed_items = self.db.feed_item.select_all().await?;
        let subscribers = self.db.subscriber.select_all().await?;
        let subscriptions = self.db.feed_subscription.select_all().await?;

        Ok(DatabaseDump {
            feeds,
            feed_items,
            subscribers,
            subscriptions,
        })
    }
}

/// Container for a full database dump.
pub struct DatabaseDump {
    pub feeds: Vec<FeedModel>,
    pub feed_items: Vec<FeedItemModel>,
    pub subscribers: Vec<SubscriberModel>,
    pub subscriptions: Vec<FeedSubscriptionModel>,
}
