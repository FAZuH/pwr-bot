use async_trait::async_trait;
use sqlx::Error as DbError;
use sqlx::SqlitePool;

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
    async fn create_table(&self) -> Result<(), DbError>;
    async fn drop_table(&self) -> Result<(), DbError>;
    async fn delete_all(&self) -> Result<(), DbError>;
}

#[async_trait]
pub trait Table<T, ID>: TableBase {
    async fn select_all(&self) -> Result<Vec<T>, DbError>;
    async fn insert(&self, model: &T) -> Result<ID, DbError>;
    async fn select(&self, id: &ID) -> Result<T, DbError>;
    async fn update(&self, model: &T) -> Result<(), DbError>;
    async fn delete(&self, id: &ID) -> Result<(), DbError>;
    async fn replace(&self, model: &T) -> Result<ID, DbError>;
}

// ============================================================================
// FeedTable
// ============================================================================

pub struct FeedTable {
    base: BaseTable,
}

impl FeedTable {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            base: BaseTable::new(pool),
        }
    }

    pub async fn select_by_url(&self, url: &str) -> Result<FeedModel, DbError> {
        sqlx::query_as::<_, FeedModel>("SELECT * FROM feeds WHERE url = ?")
            .bind(url)
            .fetch_one(&self.base.pool)
            .await
    }

    pub async fn select_all_by_tag(&self, tag: &str) -> Result<Vec<FeedModel>, DbError> {
        sqlx::query_as::<_, FeedModel>("SELECT * FROM feeds WHERE tags LIKE ?")
            .bind(format!("%{}%", tag))
            .fetch_all(&self.base.pool)
            .await
    }

    pub async fn select_all_by_url_contains(
        &self,
        pattern: &str,
    ) -> Result<Vec<FeedModel>, DbError> {
        sqlx::query_as::<_, FeedModel>("SELECT * FROM feeds WHERE url LIKE ?")
            .bind(format!("%{}%", pattern))
            .fetch_all(&self.base.pool)
            .await
    }

    pub async fn delete_all_by_url_contains(&self, pattern: &str) -> Result<(), DbError> {
        sqlx::query("DELETE FROM feeds WHERE url LIKE ?")
            .bind(format!("%{}%", pattern))
            .execute(&self.base.pool)
            .await?;
        Ok(())
    }
}

#[async_trait]
impl TableBase for FeedTable {
    async fn create_table(&self) -> Result<(), DbError> {
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS feeds (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                url TEXT NOT NULL UNIQUE,
                description TEXT DEFAULT NULL,
                tags TEXT DEFAULT NULL
            )"#,
        )
        .execute(&self.base.pool)
        .await?;
        Ok(())
    }

    async fn drop_table(&self) -> Result<(), DbError> {
        sqlx::query("DROP TABLE IF EXISTS feeds")
            .execute(&self.base.pool)
            .await?;
        Ok(())
    }

    async fn delete_all(&self) -> Result<(), DbError> {
        sqlx::query("DELETE FROM feeds")
            .execute(&self.base.pool)
            .await?;
        Ok(())
    }
}

#[async_trait]
impl Table<FeedModel, i32> for FeedTable {
    async fn select_all(&self) -> Result<Vec<FeedModel>, DbError> {
        sqlx::query_as::<_, FeedModel>("SELECT * FROM feeds")
            .fetch_all(&self.base.pool)
            .await
    }

    async fn select(&self, id: &i32) -> Result<FeedModel, DbError> {
        sqlx::query_as::<_, FeedModel>("SELECT * FROM feeds WHERE id = ?")
            .bind(id)
            .fetch_one(&self.base.pool)
            .await
    }

    async fn insert(&self, model: &FeedModel) -> Result<i32, DbError> {
        let row: (i32,) =
            sqlx::query_as("INSERT INTO feeds (name, url, tags) VALUES (?, ?, ?) RETURNING id")
                .bind(&model.name)
                .bind(&model.url)
                .bind(&model.tags)
                .fetch_one(&self.base.pool)
                .await?;
        Ok(row.0)
    }

    async fn update(&self, model: &FeedModel) -> Result<(), DbError> {
        sqlx::query("UPDATE feeds SET name = ?, description = ?, url = ?, tags = ? WHERE id = ?")
            .bind(&model.name)
            .bind(&model.description)
            .bind(&model.url)
            .bind(&model.tags)
            .bind(model.id)
            .execute(&self.base.pool)
            .await?;
        Ok(())
    }

    async fn delete(&self, id: &i32) -> Result<(), DbError> {
        sqlx::query("DELETE FROM feeds WHERE id = ?")
            .bind(id)
            .execute(&self.base.pool)
            .await?;
        Ok(())
    }

    async fn replace(&self, model: &FeedModel) -> Result<i32, DbError> {
        let row: (i32,) = sqlx::query_as(
            "REPLACE INTO feeds (name, description, url, tags) VALUES (?, ?, ?, ?) RETURNING id",
        )
        .bind(&model.name)
        .bind(&model.description)
        .bind(&model.url)
        .bind(&model.tags)
        .fetch_one(&self.base.pool)
        .await?;
        Ok(row.0)
    }
}

// ============================================================================
// FeedVersionTable
// ============================================================================

pub struct FeedItemTable {
    base: BaseTable,
}

impl FeedItemTable {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            base: BaseTable::new(pool),
        }
    }

    /// Get the latest version for a specific feed
    pub async fn select_latest_by_feed_id(&self, feed_id: i32) -> Result<FeedItemModel, DbError> {
        sqlx::query_as::<_, FeedItemModel>(
            "SELECT * FROM feed_items WHERE feed_id = ? ORDER BY published DESC LIMIT 1",
        )
        .bind(feed_id)
        .fetch_one(&self.base.pool)
        .await
    }

    /// Get all versions for a specific feed, ordered by published date
    pub async fn select_all_by_feed_id(&self, feed_id: i32) -> Result<Vec<FeedItemModel>, DbError> {
        sqlx::query_as::<_, FeedItemModel>(
            "SELECT * FROM feed_items WHERE feed_id = ? ORDER BY published DESC",
        )
        .bind(feed_id)
        .fetch_all(&self.base.pool)
        .await
    }

    /// Delete all versions for a specific feed
    pub async fn delete_all_by_feed_id(&self, feed_id: i32) -> Result<(), DbError> {
        sqlx::query("DELETE FROM feed_items WHERE feed_id = ?")
            .bind(feed_id)
            .execute(&self.base.pool)
            .await?;
        Ok(())
    }
}

#[async_trait]
impl TableBase for FeedItemTable {
    async fn create_table(&self) -> Result<(), DbError> {
        sqlx::query(
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
        )
        .execute(&self.base.pool)
        .await?;
        Ok(())
    }

    async fn drop_table(&self) -> Result<(), DbError> {
        sqlx::query("DROP TABLE IF EXISTS feed_items")
            .execute(&self.base.pool)
            .await?;
        Ok(())
    }

    async fn delete_all(&self) -> Result<(), DbError> {
        sqlx::query("DELETE FROM feed_items")
            .execute(&self.base.pool)
            .await?;
        Ok(())
    }
}

#[async_trait]
impl Table<FeedItemModel, i32> for FeedItemTable {
    async fn select_all(&self) -> Result<Vec<FeedItemModel>, DbError> {
        sqlx::query_as::<_, FeedItemModel>("SELECT * FROM feed_items ORDER BY published DESC")
            .fetch_all(&self.base.pool)
            .await
    }

    async fn select(&self, id: &i32) -> Result<FeedItemModel, DbError> {
        sqlx::query_as::<_, FeedItemModel>("SELECT * FROM feed_items WHERE id = ?")
            .bind(id)
            .fetch_one(&self.base.pool)
            .await
    }

    async fn insert(&self, model: &FeedItemModel) -> Result<i32, DbError> {
        let row: (i32,) = sqlx::query_as(
            "INSERT INTO feed_items (feed_id, description, published) VALUES (?, ?, ?) RETURNING id",
        )
        .bind(model.feed_id)
        .bind(&model.description)
        .bind(model.published)
        .fetch_one(&self.base.pool)
        .await?;
        Ok(row.0)
    }

    async fn update(&self, model: &FeedItemModel) -> Result<(), DbError> {
        sqlx::query(
            "UPDATE feed_items SET feed_id = ?, description = ?, published = ? WHERE id = ?",
        )
        .bind(model.feed_id)
        .bind(&model.description)
        .bind(model.published)
        .bind(model.id)
        .execute(&self.base.pool)
        .await?;
        Ok(())
    }

    async fn delete(&self, id: &i32) -> Result<(), DbError> {
        sqlx::query("DELETE FROM feed_items WHERE id = ?")
            .bind(id)
            .execute(&self.base.pool)
            .await?;
        Ok(())
    }

    async fn replace(&self, model: &FeedItemModel) -> Result<i32, DbError> {
        let row: (i32,) = sqlx::query_as(
            "REPLACE INTO feed_items (feed_id, description, published) VALUES (?, ?, ?) RETURNING id",
        )
        .bind(model.feed_id)
        .bind(&model.description)
        .bind(model.published)
        .fetch_one(&self.base.pool)
        .await?;
        Ok(row.0)
    }
}

// ============================================================================
// SubscriberTable
// ============================================================================

pub struct SubscriberTable {
    base: BaseTable,
}

impl SubscriberTable {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            base: BaseTable::new(pool),
        }
    }

    pub async fn select_by_type_and_feed(
        &self,
        r#type: SubscriberType,
        feed_id: i32,
    ) -> Result<Vec<SubscriberModel>, DbError> {
        sqlx::query_as::<_, SubscriberModel>(
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
        .await
    }

    pub async fn select_by_type_and_target(
        &self,
        r#type: SubscriberType,
        target_id: &str,
    ) -> Result<SubscriberModel, DbError> {
        sqlx::query_as::<_, SubscriberModel>(
            "SELECT * FROM subscribers WHERE type = ? AND target_id = ?",
        )
        .bind(r#type)
        .bind(target_id)
        .fetch_one(&self.base.pool)
        .await
    }
}

#[async_trait]
impl TableBase for SubscriberTable {
    async fn create_table(&self) -> Result<(), DbError> {
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS subscribers (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                type TEXT NOT NULL,
                target_id TEXT NOT NULL,
                UNIQUE(type, target_id)
            )"#,
        )
        .execute(&self.base.pool)
        .await?;
        Ok(())
    }

    async fn drop_table(&self) -> Result<(), DbError> {
        sqlx::query("DROP TABLE IF EXISTS subscribers")
            .execute(&self.base.pool)
            .await?;
        Ok(())
    }

    async fn delete_all(&self) -> Result<(), DbError> {
        sqlx::query("DELETE FROM subscribers")
            .execute(&self.base.pool)
            .await?;
        Ok(())
    }
}

#[async_trait]
impl Table<SubscriberModel, i32> for SubscriberTable {
    async fn select_all(&self) -> Result<Vec<SubscriberModel>, DbError> {
        sqlx::query_as::<_, SubscriberModel>("SELECT * FROM subscribers")
            .fetch_all(&self.base.pool)
            .await
    }

    async fn select(&self, id: &i32) -> Result<SubscriberModel, DbError> {
        sqlx::query_as::<_, SubscriberModel>("SELECT * FROM subscribers WHERE id = ?")
            .bind(id)
            .fetch_one(&self.base.pool)
            .await
    }

    async fn insert(&self, model: &SubscriberModel) -> Result<i32, DbError> {
        let row: (i32,) =
            sqlx::query_as("INSERT INTO subscribers (type, target_id) VALUES (?, ?) RETURNING id")
                .bind(model.r#type)
                .bind(&model.target_id)
                .fetch_one(&self.base.pool)
                .await?;
        Ok(row.0)
    }

    async fn update(&self, model: &SubscriberModel) -> Result<(), DbError> {
        sqlx::query("UPDATE subscribers SET type = ?, target_id = ? WHERE id = ?")
            .bind(model.r#type)
            .bind(&model.target_id)
            .bind(model.id)
            .execute(&self.base.pool)
            .await?;
        Ok(())
    }

    async fn delete(&self, id: &i32) -> Result<(), DbError> {
        sqlx::query("DELETE FROM subscribers WHERE id = ?")
            .bind(id)
            .execute(&self.base.pool)
            .await?;
        Ok(())
    }

    async fn replace(&self, model: &SubscriberModel) -> Result<i32, DbError> {
        let row: (i32,) =
            sqlx::query_as("REPLACE INTO subscribers (type, target_id) VALUES (?, ?) RETURNING id")
                .bind(model.r#type)
                .bind(&model.target_id)
                .fetch_one(&self.base.pool)
                .await?;
        Ok(row.0)
    }
}

// ============================================================================
// FeedSubscriptionTable
// ============================================================================

pub struct FeedSubscriptionTable {
    base: BaseTable,
}

impl FeedSubscriptionTable {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            base: BaseTable::new(pool),
        }
    }

    /// Get all subscribers for a specific feed
    pub async fn select_all_by_feed_id(
        &self,
        feed_id: i32,
    ) -> Result<Vec<FeedSubscriptionModel>, DbError> {
        sqlx::query_as::<_, FeedSubscriptionModel>(
            "SELECT * FROM feed_subscriptions WHERE feed_id = ?",
        )
        .bind(feed_id)
        .fetch_all(&self.base.pool)
        .await
    }

    /// Get all feeds a subscriber is following
    pub async fn select_all_by_subscriber_id(
        &self,
        subscriber_id: i32,
    ) -> Result<Vec<FeedSubscriptionModel>, DbError> {
        sqlx::query_as::<_, FeedSubscriptionModel>(
            "SELECT * FROM feed_subscriptions WHERE subscriber_id = ?",
        )
        .bind(subscriber_id)
        .fetch_all(&self.base.pool)
        .await
    }

    /// Check if a subscription exists
    pub async fn exists_by_feed_id(&self, feed_id: i32) -> Result<bool, DbError> {
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
    ) -> Result<bool, DbError> {
        let res =
            sqlx::query("DELETE FROM feed_subscriptions WHERE feed_id = ? AND subscriber_id = ?")
                .bind(feed_id)
                .bind(subscriber_id)
                .execute(&self.base.pool)
                .await?;
        Ok(res.rows_affected() > 0)
    }

    /// Delete all subscriptions for a feed
    pub async fn delete_all_by_feed_id(&self, feed_id: i32) -> Result<(), DbError> {
        sqlx::query("DELETE FROM feed_subscriptions WHERE feed_id = ?")
            .bind(feed_id)
            .execute(&self.base.pool)
            .await?;
        Ok(())
    }

    /// Delete all subscriptions for a subscriber
    pub async fn delete_all_by_subscriber_id(&self, subscriber_id: i32) -> Result<(), DbError> {
        sqlx::query("DELETE FROM feed_subscriptions WHERE subscriber_id = ?")
            .bind(subscriber_id)
            .execute(&self.base.pool)
            .await?;
        Ok(())
    }
}

#[async_trait]
impl TableBase for FeedSubscriptionTable {
    async fn create_table(&self) -> Result<(), DbError> {
        sqlx::query(
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
        )
        .execute(&self.base.pool)
        .await?;
        Ok(())
    }

    async fn drop_table(&self) -> Result<(), DbError> {
        sqlx::query("DROP TABLE IF EXISTS feed_subscriptions")
            .execute(&self.base.pool)
            .await?;
        Ok(())
    }

    async fn delete_all(&self) -> Result<(), DbError> {
        sqlx::query("DELETE FROM feed_subscriptions")
            .execute(&self.base.pool)
            .await?;
        Ok(())
    }
}

#[async_trait]
impl Table<FeedSubscriptionModel, i32> for FeedSubscriptionTable {
    async fn select_all(&self) -> Result<Vec<FeedSubscriptionModel>, DbError> {
        sqlx::query_as::<_, FeedSubscriptionModel>("SELECT * FROM feed_subscriptions")
            .fetch_all(&self.base.pool)
            .await
    }

    async fn select(&self, id: &i32) -> Result<FeedSubscriptionModel, DbError> {
        sqlx::query_as::<_, FeedSubscriptionModel>("SELECT * FROM feed_subscriptions WHERE id = ?")
            .bind(id)
            .fetch_one(&self.base.pool)
            .await
    }

    async fn insert(&self, model: &FeedSubscriptionModel) -> Result<i32, DbError> {
        let row: (i32,) = sqlx::query_as(
            "INSERT INTO feed_subscriptions (feed_id, subscriber_id) VALUES (?, ?) RETURNING id",
        )
        .bind(model.feed_id)
        .bind(model.subscriber_id)
        .fetch_one(&self.base.pool)
        .await?;
        Ok(row.0)
    }

    async fn update(&self, model: &FeedSubscriptionModel) -> Result<(), DbError> {
        sqlx::query("UPDATE feed_subscriptions SET feed_id = ?, subscriber_id = ? WHERE id = ?")
            .bind(model.feed_id)
            .bind(model.subscriber_id)
            .bind(model.id)
            .execute(&self.base.pool)
            .await?;
        Ok(())
    }

    async fn delete(&self, id: &i32) -> Result<(), DbError> {
        sqlx::query("DELETE FROM feed_subscriptions WHERE id = ?")
            .bind(id)
            .execute(&self.base.pool)
            .await?;
        Ok(())
    }

    async fn replace(&self, model: &FeedSubscriptionModel) -> Result<i32, DbError> {
        let row: (i32,) = sqlx::query_as(
            "REPLACE INTO feed_subscriptions (feed_id, subscriber_id) VALUES (?, ?) RETURNING id",
        )
        .bind(model.feed_id)
        .bind(model.subscriber_id)
        .fetch_one(&self.base.pool)
        .await?;
        Ok(row.0)
    }
}
