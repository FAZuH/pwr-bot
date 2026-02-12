//! Feed subscription management service.

use std::sync::Arc;

// TODO: Improve error handling here in general
// Especially with db results
use sqlx::error::ErrorKind;

use crate::database::Database;
use crate::database::error::DatabaseError;
use crate::database::model::FeedItemModel;
use crate::database::model::FeedModel;
use crate::database::model::FeedSubscriptionModel;
use crate::database::model::ServerSettings;
use crate::database::model::ServerSettingsModel;
use crate::database::model::SubscriberModel;
use crate::database::model::SubscriberType;
use crate::database::table::Table;
use crate::error::AppError;
use crate::feed::PlatformInfo;
use crate::feed::error::FeedError;
use crate::feed::platforms::Platforms;
use crate::service::error::ServiceError;

/// Service for managing feed subscriptions and updates.
pub struct FeedSubscriptionService {
    pub db: Arc<Database>,
    pub platforms: Arc<Platforms>,
}

impl FeedSubscriptionService {
    /// Creates a new feed subscription service.
    pub fn new(db: Arc<Database>, platforms: Arc<Platforms>) -> Self {
        Self { db, platforms }
    }
    /// Core subscription operations
    ///
    /// # Performance
    /// * DB calls: 1
    pub async fn subscribe(
        &self,
        url: &str,
        subscriber: &SubscriberModel,
    ) -> Result<SubscribeResult, ServiceError> {
        let feed = self.get_or_create_feed(url).await?;

        // DB 1
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
    /// # Performance
    /// * DB calls: 1
    pub async fn get_feeds_by_tag(&self, tag: &str) -> Result<Vec<FeedModel>, ServiceError> {
        Ok(self.db.feed_table.select_all_by_tag(tag).await?)
    }

    /// Check for updates on a specific feed
    pub async fn check_feed_update(
        &self,
        feed: &FeedModel,
    ) -> Result<FeedUpdateResult, ServiceError> {
        // Skip feeds with no subscribers
        let subs = self
            .db
            .feed_subscription_table
            .exists_by_feed_id(feed.id)
            .await?;

        if !subs {
            return Ok(FeedUpdateResult::NoUpdate);
        }

        // Get the latest known version for this feed
        let old_latest = self
            .db
            .feed_item_table
            .select_latest_by_feed_id(feed.id)
            .await?;

        let platform = self
            .platforms
            .get_platform_by_source_url(&feed.source_url)
            .ok_or_else(|| {
                ServiceError::DatabaseError(DatabaseError::AppError(AppError::internal_with_ref(
                    "Series feed source with url {} not found.",
                )))
            })?;

        // Fetch current state from source
        let new_latest = match platform.fetch_latest(&feed.items_id).await {
            Ok(series) => series,
            Err(e) => {
                if matches!(e, FeedError::SourceFinished { .. }) {
                    self.db.feed_table.delete(&feed.id).await?;
                    return Ok(FeedUpdateResult::SourceFinished);
                } else {
                    return Err(e.into());
                }
            }
        };

        // Check if version changed
        if old_latest
            .as_ref()
            .is_some_and(|e| new_latest.title == e.description)
        {
            return Ok(FeedUpdateResult::NoUpdate);
        }

        // Insert new version into database
        let new_feed_item = FeedItemModel {
            id: 0,
            feed_id: feed.id,
            description: new_latest.title.clone(),
            published: new_latest.published,
        };
        self.db.feed_item_table.replace(&new_feed_item).await?;

        Ok(FeedUpdateResult::Updated {
            feed: feed.clone(),
            old_item: old_latest,
            new_item: new_feed_item,
            feed_info: platform.get_base().info.clone(),
        })
    }

    /// # Performance
    /// * DB calls: 1 + 1?
    pub async fn unsubscribe(
        &self,
        source_url: &str,
        subscriber: &SubscriberModel,
    ) -> Result<UnsubscribeResult, ServiceError> {
        // DB 1
        let feed = match self.get_feed_by_source_url(source_url).await? {
            Some(feed) => feed,
            None => {
                return Ok(UnsubscribeResult::NoneSubscribed {
                    url: source_url.to_string(),
                });
            }
        };

        // DB 1?
        match self
            .db
            .feed_subscription_table
            .delete_subscription(feed.id, subscriber.id)
            .await
        {
            Ok(not_already_deleted) => {
                if not_already_deleted {
                    Ok(UnsubscribeResult::Success { feed })
                } else {
                    Ok(UnsubscribeResult::AlreadyUnsubscribed { feed })
                }
            }
            Err(err) => Err(ServiceError::UnexpectedResult {
                message: err.to_string(),
            }),
        }
    }

    /// # Performance
    /// * DB calls: 1
    ///
    /// Where N is number of subscriptions found for given page
    pub async fn list_paginated_subscriptions(
        &self,
        subscriber: &SubscriberModel,
        page: impl Into<u32>,
        per_page: impl Into<u32>,
    ) -> Result<Vec<Subscription>, ServiceError> {
        let page = page.into() - 1;

        // DB 1
        let rows = self
            .db
            .feed_subscription_table
            .select_paginated_with_latest_by_subscriber_id(subscriber.id, page, per_page)
            .await?;

        let ret = rows
            .into_iter()
            .map(|row| {
                let feed = FeedModel {
                    id: row.id,
                    name: row.name,
                    description: row.description,
                    platform_id: row.platform_id,
                    source_id: row.source_id,
                    items_id: row.items_id,
                    source_url: row.source_url,
                    cover_url: row.cover_url,
                    tags: row.tags,
                };

                let feed_latest = if let (Some(id), Some(desc), Some(pub_date)) =
                    (row.item_id, row.item_description, row.item_published)
                {
                    Some(FeedItemModel {
                        id,
                        feed_id: feed.id,
                        description: desc,
                        published: pub_date,
                    })
                } else {
                    None
                };

                Subscription { feed, feed_latest }
            })
            .collect();

        Ok(ret)
    }

    /// # Performance
    /// * DB calls: 1
    pub async fn get_subscription_count(
        &self,
        subscriber: &SubscriberModel,
    ) -> Result<u32, ServiceError> {
        // DB 1
        Ok(self
            .db
            .feed_subscription_table
            .count_by_subscriber_id(subscriber.id)
            .await?)
    }

    /// # Performance
    /// * DB calls: 1
    pub async fn search_subcriptions(
        &self,
        subscriber: &SubscriberModel,
        partial: &str,
    ) -> Result<Vec<FeedModel>, ServiceError> {
        // DB 1
        Ok(self
            .db
            .feed_table
            .select_by_name_and_subscriber_id(&subscriber.id, partial, 25)
            .await?)
    }

    /// # Performance
    /// * DB calls: 1 + 1? + 1??
    /// * API calls: 2?
    pub async fn get_or_create_feed(&self, source_url: &str) -> Result<FeedModel, ServiceError> {
        let platform = self
            .platforms
            .get_platform_by_source_url(source_url)
            .ok_or_else(|| FeedError::UnsupportedUrl {
                url: source_url.to_string(),
            })?;
        let source_id = platform.get_id_from_source_url(source_url)?;

        // DB 1
        let feed = match self
            .db
            .feed_table
            .select_by_source_id(platform.get_id(), source_id)
            .await?
        {
            Some(res) => res,
            None => {
                // Feed doesn't exist, create it
                // API 1?
                let feed_source = platform.fetch_source(source_id).await?;

                let mut feed = FeedModel {
                    id: 0,
                    name: feed_source.name,
                    description: feed_source.description,
                    platform_id: platform.get_id().to_string(),
                    source_id: source_id.to_string(),
                    items_id: feed_source.items_id,
                    source_url: feed_source.source_url,
                    cover_url: feed_source.image_url.unwrap_or("".to_string()),
                    tags: platform.get_info().tags.clone(),
                };
                // DB 1?
                feed.id = self.db.feed_table.insert(&feed).await?;

                // API 1?
                if let Ok(feed_latest) = platform.fetch_latest(&feed.items_id).await {
                    // Create initial version
                    let version = FeedItemModel {
                        id: 0,
                        feed_id: feed.id,
                        description: feed_latest.title,
                        published: feed_latest.published,
                    };
                    // DB 1??
                    self.db.feed_item_table.insert(&version).await?;
                }

                feed
            }
        };
        Ok(feed)
    }

    /// # Performance
    /// * DB calls: 1 + 1?
    pub async fn get_or_create_subscriber(
        &self,
        target: &SubscriberTarget,
    ) -> Result<SubscriberModel, ServiceError> {
        // DB 1
        let subscriber = match self
            .db
            .subscriber_table
            .select_by_type_and_target(&target.subscriber_type, &target.target_id)
            .await?
        {
            Some(res) => res,
            None => {
                // Subscriber doesn't exist, create it
                let mut subscriber = SubscriberModel {
                    r#type: target.subscriber_type,
                    target_id: target.target_id.clone(),
                    ..Default::default()
                };
                // DB 1?
                subscriber.id = self.db.subscriber_table.insert(&subscriber).await?;
                subscriber
            }
        };
        Ok(subscriber)
    }

    /// Get [`FeedModel`] by source url.
    ///
    /// Returns `Some(FeedModel)` if found. `None` otherwise.
    ///
    /// # Performance
    /// * DB calls: 1
    pub async fn get_feed_by_source_url(
        &self,
        source_url: &str,
    ) -> Result<Option<FeedModel>, ServiceError> {
        let platform = self
            .platforms
            .get_platform_by_source_url(source_url)
            .ok_or_else(|| FeedError::UnsupportedUrl {
                url: source_url.to_string(),
            })?;
        let source_id = platform.get_id_from_source_url(source_url)?;

        // DB 1
        Ok(self
            .db
            .feed_table
            .select_by_source_id(platform.get_id(), source_id)
            .await?)
    }

    /// # Performance
    /// * DB calls: 1
    pub async fn get_server_settings(&self, guild_id: u64) -> Result<ServerSettings, ServiceError> {
        // DB 1
        match self.db.server_settings_table.select(&guild_id).await? {
            Some(model) => Ok(model.settings.0),
            None => Ok(ServerSettings::default()),
        }
    }

    /// # Performance
    /// * DB calls: 1
    pub async fn update_server_settings(
        &self,
        guild_id: u64,
        settings: ServerSettings,
    ) -> Result<(), ServiceError> {
        let model = ServerSettingsModel {
            guild_id,
            settings: sqlx::types::Json(settings),
        };
        // DB 1
        self.db.server_settings_table.replace(&model).await?;
        Ok(())
    }

    /// # Performance
    /// * DB calls: 1
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

#[derive(Clone)]
pub struct Subscription {
    pub feed: FeedModel,
    pub feed_latest: Option<FeedItemModel>,
}

#[allow(clippy::large_enum_variant)]
pub enum FeedUpdateResult {
    NoUpdate,
    Updated {
        feed: FeedModel,
        old_item: Option<FeedItemModel>,
        new_item: FeedItemModel,
        feed_info: PlatformInfo,
    },
    SourceFinished,
}
