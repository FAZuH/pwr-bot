//! Administrative and maintenance service.

use std::sync::Arc;

use crate::model::FeedItemModel;
use crate::model::FeedModel;
use crate::model::FeedSubscriptionModel;
use crate::model::SubscriberModel;
use crate::repository::Repository;
use crate::repository::table::Table;

/// Service for administrative and maintenance tasks.
pub struct MaintenanceService {
    db: Arc<Repository>,
}

impl MaintenanceService {
    /// Creates a new maintenance service.
    pub fn new(db: Arc<Repository>) -> Self {
        Self { db }
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
