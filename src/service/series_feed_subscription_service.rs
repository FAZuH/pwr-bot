use std::sync::Arc;

// TODO: Improve error handling here in general
// Especially with db results
use sqlx::error::ErrorKind;

use crate::database::Database;
use crate::database::error::DatabaseError;
use crate::database::model::FeedItemModel;
use crate::database::model::FeedModel;
use crate::database::model::FeedSubscriptionModel;
use crate::database::model::SubscriberModel;
use crate::database::model::SubscriberType;
use crate::database::table::Table;
use crate::feed::error::FeedError;
use crate::feed::error::SeriesFeedError;
use crate::feed::feeds::Feeds;
use crate::service::error::ServiceError;

pub struct SeriesFeedSubscriptionService {
    pub db: Arc<Database>,
    pub feeds: Arc<Feeds>,
}

impl SeriesFeedSubscriptionService {
    // Core subscription operations
    pub async fn subscribe(
        &self,
        url: &str,
        target: SubscriberTarget,
    ) -> Result<SubscribeResult, ServiceError> {
        let feed = self.get_or_create_feed(url).await?;
        let subscriber = self.get_or_create_subscriber(&target).await?;

        match self.create_subscription(feed.id, subscriber.id).await {
            Ok(_) => Ok(SubscribeResult::Success { feed }),
            Err(err) => {
                if let ServiceError::DatabaseError(DatabaseError::BackendError(sqlx_err)) = &err
                    && let Some(db_err) = sqlx_err.as_database_error()
                    && matches!(db_err.kind(), ErrorKind::UniqueViolation)
                {
                    Ok(SubscribeResult::AlreadySubscribed { feed })
                } else {
                    Err(ServiceError::UnexpectedResult {
                        message: err.to_string(),
                    })
                }
            }
        }
    }
    pub async fn unsubscribe(
        &self,
        url: &str,
        target: SubscriberTarget,
    ) -> Result<UnsubscribeResult, ServiceError> {
        let source =
            self.feeds
                .get_feed_by_url(url)
                .ok_or_else(|| SeriesFeedError::UnsupportedUrl {
                    url: url.to_string(),
                })?;
        let id = source
            .get_id_from_url(url)
            .map_err(SeriesFeedError::UrlParseFailed)?;
        let normalized_url = source.get_url_from_id(id);

        let feed = match self.db.feed_table.select_by_url(&normalized_url).await {
            Ok(feed) => feed,
            Err(err) => {
                if let DatabaseError::BackendError(sqlx::Error::RowNotFound) = &err {
                    return Ok(UnsubscribeResult::NoneSubscribed {
                        url: url.to_string(),
                    });
                } else {
                    return Err(ServiceError::UnexpectedResult {
                        message: err.to_string(),
                    });
                }
            }
        };

        let subscriber = self.get_or_create_subscriber(&target).await?;

        match self
            .db
            .feed_subscription_table
            .delete_subscription(feed.id, subscriber.id)
            .await
        {
            Ok(_) => Ok(UnsubscribeResult::Success { feed }),
            Err(err) => {
                if let DatabaseError::BackendError(sqlx::Error::RowNotFound) = &err {
                    Ok(UnsubscribeResult::AlreadyUnsubscribed { feed })
                } else {
                    Err(ServiceError::UnexpectedResult {
                        message: err.to_string(),
                    })
                }
            }
        }
    }
    pub async fn list_paginated_subscriptions(
        &self,
        target: &SubscriberTarget,
        page: impl Into<u32>,
        per_page: impl Into<u32>,
    ) -> Result<Vec<SubscriptionInfo>, ServiceError> {
        let subscriber = self.get_or_create_subscriber(&target).await?;

        let mut ret = vec![];
        // Existence guaranteed by FOREIGN KEY constraint
        let subscriptions = self
            .db
            .feed_subscription_table
            .select_paginated_by_subscriber_id(subscriber.id, page, per_page)
            .await?;
        for sub in subscriptions {
            // Existence guaranteed by FOREIGN KEY constraint
            let feed = self.db.feed_table.select(&sub.feed_id).await?;
            let feed_latest = self
                .db
                .feed_item_table
                .select_latest_by_feed_id(feed.id)
                .await?;

            ret.push(SubscriptionInfo { feed, feed_latest });
        }
        Ok(ret)
    }

    pub async fn get_subscription_count(&self, subscriber_id: i32) -> Result<u32, ServiceError> {
        Ok(self
            .db
            .feed_subscription_table
            .count_by_subscriber_id(subscriber_id)
            .await?)
    }

    async fn get_or_create_feed(&self, url: &str) -> Result<FeedModel, ServiceError> {
        let feed = match self.db.feed_table.select_by_url(url).await {
            Ok(res) => res,
            Err(_) => {
                // Feed doesn't exist, create it
                let series_latest = self.feeds.get_latest_by_url(url).await?;
                let series_info = self.feeds.get_info_by_url(url).await?;

                let mut feed = FeedModel {
                    name: series_info.title,
                    description: series_info.description,
                    url: series_info.url,
                    cover_url: series_info.cover_url.unwrap_or("".to_string()),
                    tags: "series".to_string(),
                    ..Default::default()
                };
                feed.id = self.db.feed_table.insert(&feed).await?;

                // Create initial version
                let version = FeedItemModel {
                    feed_id: feed.id,
                    description: series_latest.latest,
                    published: series_latest.published,
                    ..Default::default()
                };
                self.db.feed_item_table.insert(&version).await?;

                feed
            }
        };
        Ok(feed)
    }
    pub async fn get_or_create_subscriber(
        &self,
        target: &SubscriberTarget,
    ) -> Result<SubscriberModel, ServiceError> {
        let subscriber = match self
            .db
            .subscriber_table
            .select_by_type_and_target(&target.subscriber_type, &target.target_id)
            .await
        {
            Ok(res) => res,
            Err(_) => {
                // Subscriber doesn't exist, create it
                let subscriber = SubscriberModel {
                    r#type: target.subscriber_type,
                    target_id: target.target_id.clone(),
                    ..Default::default()
                };
                self.db.subscriber_table.insert(&subscriber).await?;
                subscriber
            }
        };
        Ok(subscriber)
    }

    async fn create_subscription(
        &self,
        feed_id: i32,
        subscriber_id: i32,
    ) -> Result<(), ServiceError> {
        let subscription = FeedSubscriptionModel {
            feed_id,
            subscriber_id,
            ..Default::default()
        };
        self.db
            .feed_subscription_table
            .insert(&subscription)
            .await?;
        Ok(())
    }
}

// Return types
pub enum SubscribeResult {
    /// Successfully subscribed from feed
    Success { feed: FeedModel },
    /// Already subscribed from feed
    AlreadySubscribed { feed: FeedModel },
}

pub enum UnsubscribeResult {
    /// Successfully unsubscribed from feed
    Success { feed: FeedModel },
    /// Already unsubscribed from feed
    AlreadyUnsubscribed { feed: FeedModel },
    /// The url is not found in the app database, i.e., the url has not been subscribed by anyone
    NoneSubscribed { url: String },
}

pub struct SubscriberTarget {
    pub subscriber_type: SubscriberType, // Guild or Dm
    pub target_id: String,               // "guild_id:channel_id" or "user_id"
}

pub struct SubscriptionInfo {
    pub feed: FeedModel,
    pub feed_latest: FeedItemModel,
}

impl From<SeriesFeedError> for ServiceError {
    fn from(err: SeriesFeedError) -> Self {
        ServiceError::FeedError(FeedError::SeriesFeedError(err))
    }
}
