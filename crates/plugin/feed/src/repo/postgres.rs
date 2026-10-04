//! PostgreSQL database operations and implementations.

use diesel::prelude::*;
use diesel_async::RunQueryDsl;

use crate::entity::FeedEntity;
use crate::entity::FeedItemEntity;
use crate::entity::FeedSubscriptionEntity;
use crate::entity::FeedWithLatestItemRow;
use crate::entity::SubscriberEntity;
use crate::entity::SubscriberType;
use crate::repo::error::DatabaseError;
use crate::repo::schema::feed_items;
use crate::repo::schema::feed_subscriptions;
use crate::repo::schema::feeds;
use crate::repo::schema::subscribers;
use crate::repo::traits::CrudTable;
use crate::repo::traits::FeedItemRepository;
use crate::repo::traits::FeedRepository;
use crate::repo::traits::FeedSubscriptionRepository;
use crate::repo::traits::SubscriberRepository;
use crate::repo::traits::TableBase;
use crate::storage::DbPool;

macro_rules! impl_table_base {
    ($struct_name:ident, $table:path) => {
        #[async_trait::async_trait]
        impl TableBase for $struct_name {
            async fn create_table(&self) -> Result<(), DatabaseError> {
                Ok(())
            }

            async fn delete_all(&self) -> Result<(), DatabaseError> {
                let mut conn = self.pool.get().await?;
                diesel::delete($table).execute(&mut conn).await?;
                Ok(())
            }
        }
    };
}

// ============================================================================
// PgFeedRepo
// ============================================================================

#[derive(Clone)]
pub struct PgFeedRepo {
    pool: DbPool,
}

impl PgFeedRepo {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }
}

impl_table_base!(PgFeedRepo, feeds::table);

#[async_trait::async_trait]
impl CrudTable<FeedEntity, i32> for PgFeedRepo {
    async fn select_all(&self) -> Result<Vec<FeedEntity>, DatabaseError> {
        let mut conn = self.pool.get().await?;
        Ok(feeds::table
            .select(FeedEntity::as_select())
            .load(&mut conn)
            .await?)
    }

    async fn insert(&self, model: &FeedEntity) -> Result<i32, DatabaseError> {
        let mut conn = self.pool.get().await?;
        let id = diesel::insert_into(feeds::table)
            .values((
                feeds::name.eq(&model.name),
                feeds::description.eq(&model.description),
                feeds::platform_id.eq(&model.platform_id),
                feeds::source_id.eq(&model.source_id),
                feeds::items_id.eq(&model.items_id),
                feeds::source_url.eq(&model.source_url),
                feeds::cover_url.eq(&model.cover_url),
                feeds::tags.eq(&model.tags),
            ))
            .returning(feeds::id)
            .get_result(&mut conn)
            .await?;
        Ok(id)
    }

    async fn select(&self, id: &i32) -> Result<Option<FeedEntity>, DatabaseError> {
        let mut conn = self.pool.get().await?;
        Ok(feeds::table
            .find(id)
            .select(FeedEntity::as_select())
            .first(&mut conn)
            .await
            .optional()?)
    }

    async fn update(&self, model: &FeedEntity) -> Result<(), DatabaseError> {
        let mut conn = self.pool.get().await?;
        diesel::update(feeds::table.find(model.id))
            .set((
                feeds::name.eq(&model.name),
                feeds::description.eq(&model.description),
                feeds::platform_id.eq(&model.platform_id),
                feeds::source_id.eq(&model.source_id),
                feeds::items_id.eq(&model.items_id),
                feeds::source_url.eq(&model.source_url),
                feeds::cover_url.eq(&model.cover_url),
                feeds::tags.eq(&model.tags),
            ))
            .execute(&mut conn)
            .await?;
        Ok(())
    }

    async fn delete(&self, id: &i32) -> Result<(), DatabaseError> {
        let mut conn = self.pool.get().await?;
        diesel::delete(feeds::table.find(id))
            .execute(&mut conn)
            .await?;
        Ok(())
    }

    async fn replace(&self, model: &FeedEntity) -> Result<i32, DatabaseError> {
        if model.id != 0 && self.select(&model.id).await?.is_some() {
            self.update(model).await?;
            return Ok(model.id);
        }
        self.insert(model).await
    }
}

#[async_trait::async_trait]
impl FeedRepository for PgFeedRepo {
    async fn select_all_by_tag(&self, tag: &str) -> Result<Vec<FeedEntity>, DatabaseError> {
        let mut conn = self.pool.get().await?;
        let pattern = format!("%{tag}%");
        Ok(feeds::table
            .filter(feeds::tags.like(pattern))
            .select(FeedEntity::as_select())
            .load(&mut conn)
            .await?)
    }

    async fn select_by_source_id(
        &self,
        platform_id: &str,
        source_id: &str,
    ) -> Result<Option<FeedEntity>, DatabaseError> {
        let mut conn = self.pool.get().await?;
        Ok(feeds::table
            .filter(feeds::platform_id.eq(platform_id))
            .filter(feeds::source_id.eq(source_id))
            .select(FeedEntity::as_select())
            .first(&mut conn)
            .await
            .optional()?)
    }

    async fn select_by_name_and_subscriber_id(
        &self,
        subscriber_id: &i32,
        name_search: &str,
        limit: Option<u32>,
    ) -> Result<Vec<FeedEntity>, DatabaseError> {
        let mut conn = self.pool.get().await?;
        let limit = limit.unwrap_or(25) as i64;
        let pattern = format!("%{}%", name_search.to_lowercase());

        Ok(feeds::table
            .filter(
                feeds::name.ilike(pattern).and(
                    feeds::id.eq_any(
                        feed_subscriptions::table
                            .filter(feed_subscriptions::subscriber_id.eq(subscriber_id))
                            .select(feed_subscriptions::feed_id),
                    ),
                ),
            )
            .order(feeds::name.asc())
            .limit(limit)
            .select(FeedEntity::as_select())
            .load(&mut conn)
            .await?)
    }
}

// ============================================================================
// PgFeedItemRepo
// ============================================================================

#[derive(Clone)]
pub struct PgFeedItemRepo {
    pool: DbPool,
}

impl PgFeedItemRepo {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }
}

impl_table_base!(PgFeedItemRepo, feed_items::table);

#[async_trait::async_trait]
impl CrudTable<FeedItemEntity, i32> for PgFeedItemRepo {
    async fn select_all(&self) -> Result<Vec<FeedItemEntity>, DatabaseError> {
        let mut conn = self.pool.get().await?;
        Ok(feed_items::table
            .select(FeedItemEntity::as_select())
            .load(&mut conn)
            .await?)
    }

    async fn insert(&self, model: &FeedItemEntity) -> Result<i32, DatabaseError> {
        let mut conn = self.pool.get().await?;
        let id = diesel::insert_into(feed_items::table)
            .values((
                feed_items::feed_id.eq(model.feed_id),
                feed_items::description.eq(&model.description),
                feed_items::published.eq(model.published),
            ))
            .returning(feed_items::id)
            .get_result(&mut conn)
            .await?;
        Ok(id)
    }

    async fn select(&self, id: &i32) -> Result<Option<FeedItemEntity>, DatabaseError> {
        let mut conn = self.pool.get().await?;
        Ok(feed_items::table
            .find(id)
            .select(FeedItemEntity::as_select())
            .first(&mut conn)
            .await
            .optional()?)
    }

    async fn update(&self, model: &FeedItemEntity) -> Result<(), DatabaseError> {
        let mut conn = self.pool.get().await?;
        diesel::update(feed_items::table.find(model.id))
            .set((
                feed_items::feed_id.eq(model.feed_id),
                feed_items::description.eq(&model.description),
                feed_items::published.eq(model.published),
            ))
            .execute(&mut conn)
            .await?;
        Ok(())
    }

    async fn delete(&self, id: &i32) -> Result<(), DatabaseError> {
        let mut conn = self.pool.get().await?;
        diesel::delete(feed_items::table.find(id))
            .execute(&mut conn)
            .await?;
        Ok(())
    }

    async fn replace(&self, model: &FeedItemEntity) -> Result<i32, DatabaseError> {
        self.insert(model).await
    }
}

#[async_trait::async_trait]
impl FeedItemRepository for PgFeedItemRepo {
    async fn select_latest_by_feed_id(
        &self,
        feed_id: i32,
    ) -> Result<Option<FeedItemEntity>, DatabaseError> {
        let mut conn = self.pool.get().await?;
        Ok(feed_items::table
            .filter(feed_items::feed_id.eq(feed_id))
            .order(feed_items::published.desc())
            .select(FeedItemEntity::as_select())
            .first(&mut conn)
            .await
            .optional()?)
    }

    async fn select_all_by_feed_id(
        &self,
        feed_id: i32,
    ) -> Result<Vec<FeedItemEntity>, DatabaseError> {
        let mut conn = self.pool.get().await?;
        Ok(feed_items::table
            .filter(feed_items::feed_id.eq(feed_id))
            .order(feed_items::published.desc())
            .select(FeedItemEntity::as_select())
            .load(&mut conn)
            .await?)
    }

    async fn delete_all_by_feed_id(&self, feed_id: i32) -> Result<(), DatabaseError> {
        let mut conn = self.pool.get().await?;
        diesel::delete(feed_items::table.filter(feed_items::feed_id.eq(feed_id)))
            .execute(&mut conn)
            .await?;
        Ok(())
    }
}

// ============================================================================
// PgSubscriberRepo
// ============================================================================

#[derive(Clone)]
pub struct PgSubscriberRepo {
    pool: DbPool,
}

impl PgSubscriberRepo {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }
}

impl_table_base!(PgSubscriberRepo, subscribers::table);

#[async_trait::async_trait]
impl CrudTable<SubscriberEntity, i32> for PgSubscriberRepo {
    async fn select_all(&self) -> Result<Vec<SubscriberEntity>, DatabaseError> {
        let mut conn = self.pool.get().await?;
        Ok(subscribers::table
            .select(SubscriberEntity::as_select())
            .load(&mut conn)
            .await?)
    }

    async fn insert(&self, model: &SubscriberEntity) -> Result<i32, DatabaseError> {
        let mut conn = self.pool.get().await?;
        let id = diesel::insert_into(subscribers::table)
            .values((
                subscribers::type_.eq(model.r#type),
                subscribers::target_id.eq(&model.target_id),
            ))
            .returning(subscribers::id)
            .get_result(&mut conn)
            .await?;
        Ok(id)
    }

    async fn select(&self, id: &i32) -> Result<Option<SubscriberEntity>, DatabaseError> {
        let mut conn = self.pool.get().await?;
        Ok(subscribers::table
            .find(id)
            .select(SubscriberEntity::as_select())
            .first(&mut conn)
            .await
            .optional()?)
    }

    async fn update(&self, model: &SubscriberEntity) -> Result<(), DatabaseError> {
        let mut conn = self.pool.get().await?;
        diesel::update(subscribers::table.find(model.id))
            .set((
                subscribers::type_.eq(model.r#type),
                subscribers::target_id.eq(&model.target_id),
            ))
            .execute(&mut conn)
            .await?;
        Ok(())
    }

    async fn delete(&self, id: &i32) -> Result<(), DatabaseError> {
        let mut conn = self.pool.get().await?;
        diesel::delete(subscribers::table.find(id))
            .execute(&mut conn)
            .await?;
        Ok(())
    }

    async fn replace(&self, model: &SubscriberEntity) -> Result<i32, DatabaseError> {
        self.insert(model).await
    }
}

#[async_trait::async_trait]
impl SubscriberRepository for PgSubscriberRepo {
    async fn select_all_by_type_and_feed(
        &self,
        r#type: SubscriberType,
        feed_id: i32,
    ) -> Result<Vec<SubscriberEntity>, DatabaseError> {
        let mut conn = self.pool.get().await?;
        Ok(subscribers::table
            .filter(subscribers::type_.eq(r#type))
            .filter(
                subscribers::id.eq_any(
                    feed_subscriptions::table
                        .filter(feed_subscriptions::feed_id.eq(feed_id))
                        .select(feed_subscriptions::subscriber_id),
                ),
            )
            .select(SubscriberEntity::as_select())
            .load(&mut conn)
            .await?)
    }

    async fn select_by_type_and_target(
        &self,
        r#type: &SubscriberType,
        target_id: &str,
    ) -> Result<Option<SubscriberEntity>, DatabaseError> {
        let mut conn = self.pool.get().await?;
        Ok(subscribers::table
            .filter(subscribers::type_.eq(r#type))
            .filter(subscribers::target_id.eq(target_id))
            .select(SubscriberEntity::as_select())
            .first(&mut conn)
            .await
            .optional()?)
    }
}

// ============================================================================
// PgFeedSubscriptionRepo
// ============================================================================

#[derive(Clone)]
pub struct PgFeedSubscriptionRepo {
    pool: DbPool,
}

impl PgFeedSubscriptionRepo {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }
}

impl_table_base!(PgFeedSubscriptionRepo, feed_subscriptions::table);

#[async_trait::async_trait]
impl CrudTable<FeedSubscriptionEntity, i32> for PgFeedSubscriptionRepo {
    async fn select_all(&self) -> Result<Vec<FeedSubscriptionEntity>, DatabaseError> {
        let mut conn = self.pool.get().await?;
        Ok(feed_subscriptions::table
            .select(FeedSubscriptionEntity::as_select())
            .load(&mut conn)
            .await?)
    }

    async fn insert(&self, model: &FeedSubscriptionEntity) -> Result<i32, DatabaseError> {
        let mut conn = self.pool.get().await?;
        let id = diesel::insert_into(feed_subscriptions::table)
            .values((
                feed_subscriptions::feed_id.eq(model.feed_id),
                feed_subscriptions::subscriber_id.eq(model.subscriber_id),
            ))
            .returning(feed_subscriptions::id)
            .get_result(&mut conn)
            .await?;
        Ok(id)
    }

    async fn select(&self, id: &i32) -> Result<Option<FeedSubscriptionEntity>, DatabaseError> {
        let mut conn = self.pool.get().await?;
        Ok(feed_subscriptions::table
            .find(id)
            .select(FeedSubscriptionEntity::as_select())
            .first(&mut conn)
            .await
            .optional()?)
    }

    async fn update(&self, model: &FeedSubscriptionEntity) -> Result<(), DatabaseError> {
        let mut conn = self.pool.get().await?;
        diesel::update(feed_subscriptions::table.find(model.id))
            .set((
                feed_subscriptions::feed_id.eq(model.feed_id),
                feed_subscriptions::subscriber_id.eq(model.subscriber_id),
            ))
            .execute(&mut conn)
            .await?;
        Ok(())
    }

    async fn delete(&self, id: &i32) -> Result<(), DatabaseError> {
        let mut conn = self.pool.get().await?;
        diesel::delete(feed_subscriptions::table.find(id))
            .execute(&mut conn)
            .await?;
        Ok(())
    }

    async fn replace(&self, model: &FeedSubscriptionEntity) -> Result<i32, DatabaseError> {
        self.insert(model).await
    }
}

#[async_trait::async_trait]
impl FeedSubscriptionRepository for PgFeedSubscriptionRepo {
    async fn select_all_by_feed_id(
        &self,
        feed_id: i32,
    ) -> Result<Vec<FeedSubscriptionEntity>, DatabaseError> {
        let mut conn = self.pool.get().await?;
        Ok(feed_subscriptions::table
            .filter(feed_subscriptions::feed_id.eq(feed_id))
            .select(FeedSubscriptionEntity::as_select())
            .load(&mut conn)
            .await?)
    }

    async fn select_all_by_subscriber_id(
        &self,
        subscriber_id: i32,
    ) -> Result<Vec<FeedSubscriptionEntity>, DatabaseError> {
        let mut conn = self.pool.get().await?;
        Ok(feed_subscriptions::table
            .filter(feed_subscriptions::subscriber_id.eq(subscriber_id))
            .select(FeedSubscriptionEntity::as_select())
            .load(&mut conn)
            .await?)
    }

    async fn count_by_subscriber_id(&self, subscriber_id: i32) -> Result<u32, DatabaseError> {
        let mut conn = self.pool.get().await?;
        let count: i64 = feed_subscriptions::table
            .filter(feed_subscriptions::subscriber_id.eq(subscriber_id))
            .count()
            .get_result(&mut conn)
            .await?;
        Ok(count as u32)
    }

    async fn select_paginated_by_subscriber_id(
        &self,
        subscriber_id: i32,
        page: u32,
        per_page: u32,
    ) -> Result<Vec<FeedSubscriptionEntity>, DatabaseError> {
        let mut conn = self.pool.get().await?;
        let limit = per_page as i64;
        let offset = (per_page * page) as i64;
        Ok(feed_subscriptions::table
            .filter(feed_subscriptions::subscriber_id.eq(subscriber_id))
            .order(feed_subscriptions::id.asc())
            .limit(limit)
            .offset(offset)
            .select(FeedSubscriptionEntity::as_select())
            .load(&mut conn)
            .await?)
    }

    async fn select_paginated_with_latest_by_subscriber_id(
        &self,
        subscriber_id: i32,
        page: u32,
        per_page: u32,
    ) -> Result<Vec<FeedWithLatestItemRow>, DatabaseError> {
        let mut conn = self.pool.get().await?;
        let limit = per_page as i64;
        let offset = (per_page * page) as i64;

        let rows = diesel::sql_query(
            r#"
            SELECT
                f.id, f.name, f.description, f.platform_id, f.source_id, f.items_id,
                f.source_url, f.cover_url, f.tags,
                fi.id as item_id, fi.description as item_description, fi.published as item_published
            FROM feed_subscriptions fs
            JOIN feeds f ON fs.feed_id = f.id
            LEFT JOIN feed_items fi ON fi.id = (
                SELECT id FROM feed_items WHERE feed_id = f.id ORDER BY published DESC LIMIT 1
            )
            WHERE fs.subscriber_id = $1
            ORDER BY f.name
            LIMIT $2 OFFSET $3
            "#,
        )
        .bind::<diesel::sql_types::Integer, _>(subscriber_id)
        .bind::<diesel::sql_types::BigInt, _>(limit)
        .bind::<diesel::sql_types::BigInt, _>(offset)
        .load::<FeedWithLatestItemRow>(&mut conn)
        .await?;
        Ok(rows)
    }

    async fn exists_by_feed_id(&self, feed_id: i32) -> Result<bool, DatabaseError> {
        let mut conn = self.pool.get().await?;
        let count: i64 = feed_subscriptions::table
            .filter(feed_subscriptions::feed_id.eq(feed_id))
            .count()
            .get_result(&mut conn)
            .await?;
        Ok(count > 0)
    }

    async fn delete_subscription(
        &self,
        feed_id: i32,
        subscriber_id: i32,
    ) -> Result<bool, DatabaseError> {
        let mut conn = self.pool.get().await?;
        let affected = diesel::delete(
            feed_subscriptions::table
                .filter(feed_subscriptions::feed_id.eq(feed_id))
                .filter(feed_subscriptions::subscriber_id.eq(subscriber_id)),
        )
        .execute(&mut conn)
        .await?;
        Ok(affected > 0)
    }

    async fn delete_all_by_feed_id(&self, feed_id: i32) -> Result<(), DatabaseError> {
        let mut conn = self.pool.get().await?;
        diesel::delete(feed_subscriptions::table.filter(feed_subscriptions::feed_id.eq(feed_id)))
            .execute(&mut conn)
            .await?;
        Ok(())
    }

    async fn delete_all_by_subscriber_id(&self, subscriber_id: i32) -> Result<(), DatabaseError> {
        let mut conn = self.pool.get().await?;
        diesel::delete(
            feed_subscriptions::table.filter(feed_subscriptions::subscriber_id.eq(subscriber_id)),
        )
        .execute(&mut conn)
        .await?;
        Ok(())
    }
}
