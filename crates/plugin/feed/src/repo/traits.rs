use async_trait::async_trait;

use crate::entity::FeedEntity;
use crate::entity::FeedItemEntity;
use crate::entity::FeedSubscriptionEntity;
use crate::entity::FeedWithLatestItemRow;
use crate::entity::SubscriberEntity;
use crate::entity::SubscriberType;
use crate::repo::error::DatabaseError;

#[async_trait]
pub trait TableBase: Send + Sync {
    async fn create_table(&self) -> Result<(), DatabaseError>;
    async fn delete_all(&self) -> Result<(), DatabaseError>;
}

#[async_trait]
pub trait CrudTable<T, ID>: TableBase {
    async fn select_all(&self) -> Result<Vec<T>, DatabaseError>;
    async fn insert(&self, model: &T) -> Result<ID, DatabaseError>;
    async fn select(&self, id: &ID) -> Result<Option<T>, DatabaseError>;
    async fn update(&self, model: &T) -> Result<(), DatabaseError>;
    async fn delete(&self, id: &ID) -> Result<(), DatabaseError>;
    async fn replace(&self, model: &T) -> Result<ID, DatabaseError>;
}

#[async_trait]
pub trait FeedRepository: CrudTable<FeedEntity, i32> + Send + Sync {
    async fn select_all_by_tag(&self, tag: &str) -> Result<Vec<FeedEntity>, DatabaseError>;
    async fn select_by_source_id(
        &self,
        platform_id: &str,
        source_id: &str,
    ) -> Result<Option<FeedEntity>, DatabaseError>;
    async fn select_by_name_and_subscriber_id(
        &self,
        subscriber_id: &i32,
        name_search: &str,
        limit: Option<u32>,
    ) -> Result<Vec<FeedEntity>, DatabaseError>;
}

#[async_trait]
pub trait FeedItemRepository: CrudTable<FeedItemEntity, i32> + Send + Sync {
    async fn select_latest_by_feed_id(
        &self,
        feed_id: i32,
    ) -> Result<Option<FeedItemEntity>, DatabaseError>;
    async fn select_all_by_feed_id(
        &self,
        feed_id: i32,
    ) -> Result<Vec<FeedItemEntity>, DatabaseError>;
    async fn delete_all_by_feed_id(&self, feed_id: i32) -> Result<(), DatabaseError>;
}

#[async_trait]
pub trait SubscriberRepository: CrudTable<SubscriberEntity, i32> + Send + Sync {
    async fn select_all_by_type_and_feed(
        &self,
        r#type: SubscriberType,
        feed_id: i32,
    ) -> Result<Vec<SubscriberEntity>, DatabaseError>;
    async fn select_by_type_and_target(
        &self,
        r#type: &SubscriberType,
        target_id: &str,
    ) -> Result<Option<SubscriberEntity>, DatabaseError>;
}

#[async_trait]
pub trait FeedSubscriptionRepository: CrudTable<FeedSubscriptionEntity, i32> + Send + Sync {
    async fn select_all_by_feed_id(
        &self,
        feed_id: i32,
    ) -> Result<Vec<FeedSubscriptionEntity>, DatabaseError>;
    async fn select_all_by_subscriber_id(
        &self,
        subscriber_id: i32,
    ) -> Result<Vec<FeedSubscriptionEntity>, DatabaseError>;
    async fn count_by_subscriber_id(&self, subscriber_id: i32) -> Result<u32, DatabaseError>;
    async fn select_paginated_by_subscriber_id(
        &self,
        subscriber_id: i32,
        page: u32,
        per_page: u32,
    ) -> Result<Vec<FeedSubscriptionEntity>, DatabaseError>;
    async fn select_paginated_with_latest_by_subscriber_id(
        &self,
        subscriber_id: i32,
        page: u32,
        per_page: u32,
    ) -> Result<Vec<FeedWithLatestItemRow>, DatabaseError>;
    async fn exists_by_feed_id(&self, feed_id: i32) -> Result<bool, DatabaseError>;
    async fn delete_subscription(
        &self,
        feed_id: i32,
        subscriber_id: i32,
    ) -> Result<bool, DatabaseError>;
    async fn delete_all_by_feed_id(&self, feed_id: i32) -> Result<(), DatabaseError>;
    async fn delete_all_by_subscriber_id(&self, subscriber_id: i32) -> Result<(), DatabaseError>;
}

pub trait Repos: Send + Sync {
    fn feed(&self) -> Box<dyn FeedRepository + Send + Sync>;
    fn feed_item(&self) -> Box<dyn FeedItemRepository + Send + Sync>;
    fn subscriber(&self) -> Box<dyn SubscriberRepository + Send + Sync>;
    fn feed_subscription(&self) -> Box<dyn FeedSubscriptionRepository + Send + Sync>;
}
