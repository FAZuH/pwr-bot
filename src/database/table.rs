use async_trait::async_trait;
use sqlx::SqlitePool;

use crate::database::error::DatabaseError;
use crate::database::model::FeedItemModel;
use crate::database::model::FeedModel;
use crate::database::model::FeedSubscriptionModel;
use crate::database::model::SubscriberModel;
use crate::database::model::SubscriberType;

pub struct BaseTable {
    pub pool: SqlitePool,
}

impl BaseTable {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
pub trait TableBase {
    async fn create_table(&self) -> Result<(), DatabaseError>;
    async fn drop_table(&self) -> Result<(), DatabaseError>;
    async fn delete_all(&self) -> Result<(), DatabaseError>;
}

#[async_trait]
pub trait Table<T, ID>: TableBase {
    async fn select_all(&self) -> Result<Vec<T>, DatabaseError>;
    async fn insert(&self, model: &T) -> Result<ID, DatabaseError>;
    async fn select(&self, id: &ID) -> Result<T, DatabaseError>;
    async fn update(&self, model: &T) -> Result<(), DatabaseError>;
    async fn delete(&self, id: &ID) -> Result<(), DatabaseError>;
    async fn replace(&self, model: &T) -> Result<ID, DatabaseError>;
}

macro_rules! impl_table {
    (
        $struct_name:ident,
        $model:ty,
        $table:expr,
        $create_sql:expr,
        $cols:expr,
        $vals:expr,
        $update_set:expr,
        [ $( $field:ident ),+ ]
    ) => {
        pub struct $struct_name {
            base: BaseTable,
        }

        impl $struct_name {
            pub fn new(pool: SqlitePool) -> Self {
                Self {
                    base: BaseTable::new(pool),
                }
            }
        }

        #[async_trait]
        impl TableBase for $struct_name {
            async fn create_table(&self) -> Result<(), DatabaseError> {
                sqlx::query($create_sql)
                    .execute(&self.base.pool)
                    .await?;
                Ok(())
            }

            async fn drop_table(&self) -> Result<(), DatabaseError> {
                sqlx::query(concat!("DROP TABLE IF EXISTS ", $table))
                    .execute(&self.base.pool)
                    .await?;
                Ok(())
            }

            async fn delete_all(&self) -> Result<(), DatabaseError> {
                sqlx::query(concat!("DELETE FROM ", $table))
                    .execute(&self.base.pool)
                    .await?;
                Ok(())
            }
        }

        #[async_trait]
        impl Table<$model, i32> for $struct_name {
            async fn select_all(&self) -> Result<Vec<$model>, DatabaseError> {
                Ok(sqlx::query_as::<_, $model>(concat!("SELECT * FROM ", $table))
                    .fetch_all(&self.base.pool)
                    .await?)
            }

            async fn select(&self, id: &i32) -> Result<$model, DatabaseError> {
                Ok(
                    sqlx::query_as::<_, $model>(concat!("SELECT * FROM ", $table, " WHERE id = ? LIMIT 1"))
                        .bind(id)
                        .fetch_one(&self.base.pool)
                        .await?,
                )
            }

            async fn insert(&self, model: &$model) -> Result<i32, DatabaseError> {
                let row: (i32,) = sqlx::query_as(concat!(
                        "INSERT INTO ", $table, " (", $cols, ") VALUES (", $vals, ") RETURNING id"
                    ))
                    $( .bind(&model.$field) )+
                    .fetch_one(&self.base.pool)
                    .await?;
                Ok(row.0)
            }

            async fn update(&self, model: &$model) -> Result<(), DatabaseError> {
                sqlx::query(concat!(
                        "UPDATE ", $table, " SET ", $update_set, " WHERE id = ?"
                    ))
                    $( .bind(&model.$field) )+
                    .bind(model.id)
                    .execute(&self.base.pool)
                    .await?;
                Ok(())
            }

            async fn delete(&self, id: &i32) -> Result<(), DatabaseError> {
                sqlx::query(concat!("DELETE FROM ", $table, " WHERE id = ?"))
                    .bind(id)
                    .execute(&self.base.pool)
                    .await?;
                Ok(())
            }

            async fn replace(&self, model: &$model) -> Result<i32, DatabaseError> {
                let row: (i32,) = sqlx::query_as(concat!(
                        "REPLACE INTO ", $table, " (", $cols, ") VALUES (", $vals, ") RETURNING id"
                    ))
                    $( .bind(&model.$field) )+
                    .fetch_one(&self.base.pool)
                    .await?;
                Ok(row.0)
            }
        }
    };
}

// ============================================================================
// FeedTable
// ============================================================================

impl_table!(
    FeedTable,
    FeedModel,
    "feeds",
    r#"CREATE TABLE IF NOT EXISTS feeds (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        name TEXT NOT NULL,
        description TEXT DEFAULT NULL,
        url TEXT NOT NULL UNIQUE,
        cover_url TEXT DEFAULT NULL,
        tags TEXT DEFAULT NULL
    )"#,
    "name, description, url, cover_url, tags",
    "?, ?, ?, ?, ?",
    "name = ?, description = ?, url = ?, cover_url = ?, tags = ?",
    [name, description, url, cover_url, tags]
);

impl FeedTable {
    pub async fn select_by_url(&self, url: &str) -> Result<FeedModel, DatabaseError> {
        Ok(
            sqlx::query_as::<_, FeedModel>("SELECT * FROM feeds WHERE url = ? LIMIT 1")
                .bind(url)
                .fetch_one(&self.base.pool)
                .await?,
        )
    }

    pub async fn select_all_by_tag(&self, tag: &str) -> Result<Vec<FeedModel>, DatabaseError> {
        Ok(
            sqlx::query_as::<_, FeedModel>("SELECT * FROM feeds WHERE tags LIKE ?")
                .bind(format!("%{}%", tag))
                .fetch_all(&self.base.pool)
                .await?,
        )
    }

    pub async fn select_by_name_and_subscriber_id(
        &self,
        subscriber_id: &i32,
        name_search: &str,
        limit: impl Into<Option<u32>>,
    ) -> Result<Vec<FeedModel>, DatabaseError> {
        let limit = limit.into().unwrap_or(25);
        let search_pattern = format!("%{}%", name_search.to_lowercase());
        Ok(sqlx::query_as::<_, FeedModel>(
            r#"
                SELECT * FROM feeds 
                WHERE LOWER(name) LIKE ? 
                    AND id IN (
                        SELECT feed_id
                        FROM feed_subscriptions
                        WHERE subscriber_id = ?
                    )
                LIMIT ?
                "#,
        )
        .bind(search_pattern)
        .bind(subscriber_id)
        .bind(limit)
        .fetch_all(&self.base.pool)
        .await?)
    }
}

// ============================================================================
// FeedItemTable
// ============================================================================

impl_table!(
    FeedItemTable,
    FeedItemModel,
    "feed_items",
    r#"CREATE TABLE IF NOT EXISTS feed_items (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        feed_id INTEGER NOT NULL,
        description TEXT NOT NULL,
        published TIMESTAMP NOT NULL,
        UNIQUE(feed_id, published),
        FOREIGN KEY (feed_id) REFERENCES feeds(id)
            ON DELETE CASCADE
            ON UPDATE CASCADE
    )"#,
    "feed_id, description, published",
    "?, ?, ?",
    "feed_id = ?, description = ?, published = ?",
    [feed_id, description, published]
);

impl FeedItemTable {
    /// Get the latest version for a specific feed
    pub async fn select_latest_by_feed_id(
        &self,
        feed_id: i32,
    ) -> Result<FeedItemModel, DatabaseError> {
        Ok(sqlx::query_as::<_, FeedItemModel>(
            "SELECT * FROM feed_items WHERE feed_id = ? ORDER BY published DESC LIMIT 1",
        )
        .bind(feed_id)
        .fetch_one(&self.base.pool)
        .await?)
    }

    /// Get all versions for a specific feed, ordered by published date
    pub async fn select_all_by_feed_id(
        &self,
        feed_id: i32,
    ) -> Result<Vec<FeedItemModel>, DatabaseError> {
        Ok(sqlx::query_as::<_, FeedItemModel>(
            "SELECT * FROM feed_items WHERE feed_id = ? ORDER BY published DESC",
        )
        .bind(feed_id)
        .fetch_all(&self.base.pool)
        .await?)
    }

    /// Delete all versions for a specific feed
    pub async fn delete_all_by_feed_id(&self, feed_id: i32) -> Result<(), DatabaseError> {
        sqlx::query("DELETE FROM feed_items WHERE feed_id = ?")
            .bind(feed_id)
            .execute(&self.base.pool)
            .await?;
        Ok(())
    }
}

// ============================================================================
// SubscriberTable
// ============================================================================

impl_table!(
    SubscriberTable,
    SubscriberModel,
    "subscribers",
    r#"CREATE TABLE IF NOT EXISTS subscribers (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        type TEXT NOT NULL,
        target_id TEXT NOT NULL,
        UNIQUE(type, target_id)
    )"#,
    "type, target_id",
    "?, ?",
    "type = ?, target_id = ?",
    [r#type, target_id]
);

impl SubscriberTable {
    pub async fn select_all_by_type_and_feed(
        &self,
        r#type: SubscriberType,
        feed_id: i32,
    ) -> Result<Vec<SubscriberModel>, DatabaseError> {
        Ok(sqlx::query_as::<_, SubscriberModel>(
            r#"
            SELECT * FROM subscribers
            WHERE type = ?
                AND id IN (
                    SELECT subscriber_id
                    FROM feed_subscriptions
                    WHERE feed_id = ?
                )
            "#,
        )
        .bind(r#type)
        .bind(feed_id)
        .fetch_all(&self.base.pool)
        .await?)
    }

    pub async fn select_by_type_and_target(
        &self,
        r#type: &SubscriberType,
        target_id: &str,
    ) -> Result<SubscriberModel, DatabaseError> {
        Ok(sqlx::query_as::<_, SubscriberModel>(
            "SELECT * FROM subscribers WHERE type = ? AND target_id = ? LIMIT 1",
        )
        .bind(r#type)
        .bind(target_id)
        .fetch_one(&self.base.pool)
        .await?)
    }
}

// ============================================================================
// FeedSubscriptionTable
// ============================================================================

impl_table!(
    FeedSubscriptionTable,
    FeedSubscriptionModel,
    "feed_subscriptions",
    r#"CREATE TABLE IF NOT EXISTS feed_subscriptions (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        feed_id INTEGER NOT NULL,
        subscriber_id INTEGER NOT NULL,
        UNIQUE(feed_id, subscriber_id),
        FOREIGN KEY (feed_id) REFERENCES feeds(id)
            ON DELETE CASCADE
            ON UPDATE CASCADE,
        FOREIGN KEY (subscriber_id) REFERENCES subscribers(id)
            ON DELETE CASCADE
            ON UPDATE CASCADE
    )"#,
    "feed_id, subscriber_id",
    "?, ?",
    "feed_id = ?, subscriber_id = ?",
    [feed_id, subscriber_id]
);

impl FeedSubscriptionTable {
    /// Get all subscribers for a specific feed
    pub async fn select_all_by_feed_id(
        &self,
        feed_id: i32,
    ) -> Result<Vec<FeedSubscriptionModel>, DatabaseError> {
        Ok(sqlx::query_as::<_, FeedSubscriptionModel>(
            "SELECT * FROM feed_subscriptions WHERE feed_id = ?",
        )
        .bind(feed_id)
        .fetch_all(&self.base.pool)
        .await?)
    }

    /// Get all feeds a subscriber is following
    pub async fn select_all_by_subscriber_id(
        &self,
        subscriber_id: i32,
    ) -> Result<Vec<FeedSubscriptionModel>, DatabaseError> {
        Ok(sqlx::query_as::<_, FeedSubscriptionModel>(
            "SELECT * FROM feed_subscriptions WHERE subscriber_id = ?",
        )
        .bind(subscriber_id)
        .fetch_all(&self.base.pool)
        .await?)
    }

    /// Get count of feeds a subscriber is following
    pub async fn count_by_subscriber_id(&self, subscriber_id: i32) -> Result<u32, DatabaseError> {
        let count: (u32,) =
            sqlx::query_as("SELECT COUNT(*) FROM feed_subscriptions WHERE subscriber_id = ?")
                .bind(subscriber_id)
                .fetch_one(&self.base.pool)
                .await?;
        Ok(count.0)
    }

    /// Get a paginated list of feeds a subscriber is following
    ///
    /// # Arguments
    /// * `page` - n-th page to show. Starts at 0.
    /// * `per_page` - How many items to show per page.
    pub async fn select_paginated_by_subscriber_id(
        &self,
        subscriber_id: i32,
        page: impl Into<u32>,
        per_page: impl Into<u32>,
    ) -> Result<Vec<FeedSubscriptionModel>, DatabaseError> {
        let page: u32 = page.into();
        let per_page: u32 = per_page.into();

        let limit = per_page;
        let offset = per_page * page;

        Ok(sqlx::query_as::<_, FeedSubscriptionModel>(
            "SELECT * FROM feed_subscriptions WHERE subscriber_id = ? ORDER BY id LIMIT ? OFFSET ?",
        )
        .bind(subscriber_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.base.pool)
        .await?)
    }

    /// Check if a subscription exists
    pub async fn exists_by_feed_id(&self, feed_id: i32) -> Result<bool, DatabaseError> {
        let count: (i32,) =
            sqlx::query_as("SELECT COUNT(*) FROM feed_subscriptions WHERE feed_id = ?")
                .bind(feed_id)
                .fetch_one(&self.base.pool)
                .await?;
        Ok(count.0 > 0)
    }

    /// Delete a specific subscription
    pub async fn delete_subscription(
        &self,
        feed_id: i32,
        subscriber_id: i32,
    ) -> Result<bool, DatabaseError> {
        let res =
            sqlx::query("DELETE FROM feed_subscriptions WHERE feed_id = ? AND subscriber_id = ?")
                .bind(feed_id)
                .bind(subscriber_id)
                .execute(&self.base.pool)
                .await?;
        Ok(res.rows_affected() > 0)
    }

    /// Delete all subscriptions for a feed
    pub async fn delete_all_by_feed_id(&self, feed_id: i32) -> Result<(), DatabaseError> {
        sqlx::query("DELETE FROM feed_subscriptions WHERE feed_id = ?")
            .bind(feed_id)
            .execute(&self.base.pool)
            .await?;
        Ok(())
    }

    /// Delete all subscriptions for a subscriber
    pub async fn delete_all_by_subscriber_id(
        &self,
        subscriber_id: i32,
    ) -> Result<(), DatabaseError> {
        sqlx::query("DELETE FROM feed_subscriptions WHERE subscriber_id = ?")
            .bind(subscriber_id)
            .execute(&self.base.pool)
            .await?;
        Ok(())
    }
}
