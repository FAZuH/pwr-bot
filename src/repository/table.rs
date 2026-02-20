//! Database table operations and implementations.

use sqlx::SqlitePool;
use sqlx::sqlite::SqliteArguments;

use crate::error::AppError;
use crate::model::BotMetaModel;
use crate::model::FeedItemModel;
use crate::model::FeedModel;
use crate::model::FeedSubscriptionModel;
use crate::model::FeedWithLatestItemRow;
use crate::model::ServerSettingsModel;
use crate::model::SubscriberModel;
use crate::model::SubscriberType;
use crate::model::VoiceLeaderboardEntry;
use crate::model::VoiceLeaderboardOpt;
use crate::model::VoiceLeaderboardOptBuilder;
use crate::model::VoiceSessionsModel;
use crate::repository::error::DatabaseError;

/// Base table struct providing database pool access.
#[derive(Clone)]
pub struct BaseTable {
    pub pool: SqlitePool,
}

impl BaseTable {
    /// Creates a new base table with the given pool.
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

/// Base trait for table operations.
#[async_trait::async_trait]
pub trait TableBase {
    /// Creates the table if it doesn't exist.
    async fn create_table(&self) -> Result<(), DatabaseError>;
    /// Drops the table.
    async fn drop_table(&self) -> Result<(), DatabaseError>;
    /// Deletes all rows from the table.
    async fn delete_all(&self) -> Result<(), DatabaseError>;
}

/// Trait for tables with CRUD operations.
#[async_trait::async_trait]
pub trait Table<T, ID>: TableBase {
    async fn select_all(&self) -> Result<Vec<T>, DatabaseError>;
    async fn insert(&self, model: &T) -> Result<ID, DatabaseError>;
    async fn select(&self, id: &ID) -> Result<Option<T>, DatabaseError>;
    async fn update(&self, model: &T) -> Result<(), DatabaseError>;
    async fn delete(&self, id: &ID) -> Result<(), DatabaseError>;
    async fn replace(&self, model: &T) -> Result<ID, DatabaseError>;
}

/// Helper trait to handle binding parameters, especially for casting u64 to i64 for SQLite.
pub trait BindParam<'q> {
    fn bind_param<O>(
        self,
        query: sqlx::query::QueryAs<'q, sqlx::Sqlite, O, SqliteArguments<'q>>,
    ) -> sqlx::query::QueryAs<'q, sqlx::Sqlite, O, SqliteArguments<'q>>;
    fn bind_param_q(
        self,
        query: sqlx::query::Query<'q, sqlx::Sqlite, SqliteArguments<'q>>,
    ) -> sqlx::query::Query<'q, sqlx::Sqlite, SqliteArguments<'q>>;
}

macro_rules! impl_bind_param {
    ($t:ty) => {
        impl<'q> BindParam<'q> for $t {
            fn bind_param<O>(
                self,
                query: sqlx::query::QueryAs<'q, sqlx::Sqlite, O, SqliteArguments<'q>>,
            ) -> sqlx::query::QueryAs<'q, sqlx::Sqlite, O, SqliteArguments<'q>> {
                query.bind(self)
            }
            fn bind_param_q(
                self,
                query: sqlx::query::Query<'q, sqlx::Sqlite, SqliteArguments<'q>>,
            ) -> sqlx::query::Query<'q, sqlx::Sqlite, SqliteArguments<'q>> {
                query.bind(self)
            }
        }
    };
}

// Implement for reference types that are passed to .bind()
impl_bind_param!(&'q i32);
impl_bind_param!(&'q i64);
impl_bind_param!(&'q String);
impl_bind_param!(&'q Option<String>);
impl_bind_param!(&'q SubscriberType);
impl_bind_param!(&'q chrono::DateTime<chrono::Utc>);

// For Json
impl<'q, T: serde::Serialize + for<'a> serde::Deserialize<'a> + Send + Sync + 'static> BindParam<'q>
    for &'q sqlx::types::Json<T>
{
    fn bind_param<O>(
        self,
        query: sqlx::query::QueryAs<'q, sqlx::Sqlite, O, SqliteArguments<'q>>,
    ) -> sqlx::query::QueryAs<'q, sqlx::Sqlite, O, SqliteArguments<'q>> {
        query.bind(self)
    }
    fn bind_param_q(
        self,
        query: sqlx::query::Query<'q, sqlx::Sqlite, SqliteArguments<'q>>,
    ) -> sqlx::query::Query<'q, sqlx::Sqlite, SqliteArguments<'q>> {
        query.bind(self)
    }
}

// Special case for u64 (casting to i64)
impl<'q> BindParam<'q> for &'q u64 {
    fn bind_param<O>(
        self,
        query: sqlx::query::QueryAs<'q, sqlx::Sqlite, O, SqliteArguments<'q>>,
    ) -> sqlx::query::QueryAs<'q, sqlx::Sqlite, O, SqliteArguments<'q>> {
        query.bind(*self as i64)
    }
    fn bind_param_q(
        self,
        query: sqlx::query::Query<'q, sqlx::Sqlite, SqliteArguments<'q>>,
    ) -> sqlx::query::Query<'q, sqlx::Sqlite, SqliteArguments<'q>> {
        query.bind(*self as i64)
    }
}

macro_rules! impl_table {
    (
        $struct_name:ident,
        $model:ty,
        $table:expr,
        $pk:ident,
        $id_type:ty,
        $db_id_type:ty,
        $create_sql:expr,
        $cols:expr,
        $vals:expr,
        $update_set:expr,
        [ $( $field:ident ),+ ]
    ) => {
        #[derive(Clone)]
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

        #[async_trait::async_trait]
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

        #[async_trait::async_trait]
        impl Table<$model, $id_type> for $struct_name {
            async fn select_all(&self) -> Result<Vec<$model>, DatabaseError> {
                Ok(sqlx::query_as::<_, $model>(concat!("SELECT * FROM ", $table))
                    .fetch_all(&self.base.pool)
                    .await?)
            }

            async fn select(&self, id: &$id_type) -> Result<Option<$model>, DatabaseError> {
                let query = sqlx::query_as::<_, $model>(concat!("SELECT * FROM ", $table, " WHERE ", stringify!($pk), " = ?"));
                let query = BindParam::bind_param(id, query);
                Ok(
                    query
                        .fetch_optional(&self.base.pool)
                        .await?,
                )
            }

            async fn insert(&self, model: &$model) -> Result<$id_type, DatabaseError> {
                let mut query = sqlx::query_as(concat!(
                        "INSERT INTO ", $table, " (", $cols, ") VALUES (", $vals, ") RETURNING ", stringify!($pk)
                    ));

                $(
                    query = BindParam::bind_param(&model.$field, query);
                )+

                let row: ($db_id_type,) = query.fetch_one(&self.base.pool).await?;
                Ok(row.0 as $id_type)
            }

            async fn update(&self, model: &$model) -> Result<(), DatabaseError> {
                let mut query = sqlx::query(concat!(
                        "UPDATE ", $table, " SET ", $update_set, " WHERE ", stringify!($pk), " = ?"
                    ));

                $(
                    query = BindParam::bind_param_q(&model.$field, query);
                )+
                query = BindParam::bind_param_q(&model.$pk, query);

                query.execute(&self.base.pool).await?;
                Ok(())
            }

            async fn delete(&self, id: &$id_type) -> Result<(), DatabaseError> {
                let query = sqlx::query(concat!("DELETE FROM ", $table, " WHERE ", stringify!($pk), " = ?"));
                let query = BindParam::bind_param_q(id, query);
                query.execute(&self.base.pool).await?;
                Ok(())
            }

            async fn replace(&self, model: &$model) -> Result<$id_type, DatabaseError> {
                let mut query = sqlx::query_as(concat!(
                        "REPLACE INTO ", $table, " (", $cols, ") VALUES (", $vals, ") RETURNING ", stringify!($pk)
                    ));

                $(
                    query = BindParam::bind_param(&model.$field, query);
                )+

                let row: ($db_id_type,) = query.fetch_one(&self.base.pool).await?;
                Ok(row.0 as $id_type)
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
    id,
    i32,
    i32,
    r#"CREATE TABLE IF NOT EXISTS feeds (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        name TEXT NOT NULL,
        description TEXT DEFAULT NULL,
        platform_id TEXT NOT NULL,
        source_id TEXT NOT NULL,
        items_id TEXT NOT NULL,
        source_url TEXT NOT NULL,
        cover_url TEXT DEFAULT NULL,
        tags TEXT DEFAULT NULL,
        UNIQUE(platform_id, source_id),
        UNIQUE(source_url)
    )"#,
    "name, description, platform_id, source_id, items_id, source_url, cover_url, tags",
    "?, ?, ?, ?, ?, ?, ?, ?",
    "name = ?, description = ?, platform_id = ?, source_id = ?, items_id = ?, source_url = ?, cover_url = ?, tags = ?",
    [
        name,
        description,
        platform_id,
        source_id,
        items_id,
        source_url,
        cover_url,
        tags
    ]
);

impl FeedTable {
    pub async fn select_all_by_tag(&self, tag: &str) -> Result<Vec<FeedModel>, DatabaseError> {
        Ok(
            sqlx::query_as::<_, FeedModel>("SELECT * FROM feeds WHERE tags LIKE ?")
                .bind(format!("%{}%", tag))
                .fetch_all(&self.base.pool)
                .await?,
        )
    }

    pub async fn select_by_source_id(
        &self,
        platform_id: &str,
        source_id: &str,
    ) -> Result<Option<FeedModel>, DatabaseError> {
        Ok(sqlx::query_as::<_, FeedModel>(
            "SELECT * FROM feeds WHERE platform_id = ? AND source_id = ?",
        )
        .bind(platform_id)
        .bind(source_id)
        .fetch_optional(&self.base.pool)
        .await?)
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
                ORDER BY name
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
    id,
    i32,
    i32,
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
    ) -> Result<Option<FeedItemModel>, DatabaseError> {
        Ok(sqlx::query_as::<_, FeedItemModel>(
            "SELECT * FROM feed_items WHERE feed_id = ? ORDER BY published DESC LIMIT 1",
        )
        .bind(feed_id)
        .fetch_optional(&self.base.pool)
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
    id,
    i32,
    i32,
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
    ) -> Result<Option<SubscriberModel>, DatabaseError> {
        Ok(sqlx::query_as::<_, SubscriberModel>(
            "SELECT * FROM subscribers WHERE type = ? AND target_id = ? LIMIT 1",
        )
        .bind(r#type)
        .bind(target_id)
        .fetch_optional(&self.base.pool)
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
    id,
    i32,
    i32,
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

    /// Get a paginated list of feeds a subscriber is following, with latest feed item
    ///
    /// # Arguments
    /// * `page` - n-th page to show. Starts at 0.
    /// * `per_page` - How many items to show per page.
    pub async fn select_paginated_with_latest_by_subscriber_id(
        &self,
        subscriber_id: i32,
        page: impl Into<u32>,
        per_page: impl Into<u32>,
    ) -> Result<Vec<FeedWithLatestItemRow>, DatabaseError> {
        let page: u32 = page.into();
        let per_page: u32 = per_page.into();

        let limit = per_page;
        let offset = per_page * page;

        Ok(sqlx::query_as::<_, FeedWithLatestItemRow>(
            r#"
            SELECT 
                f.id, f.name, f.description, f.platform_id, f.source_id, f.items_id, f.source_url, f.cover_url, f.tags,
                fi.id as item_id, fi.description as item_description, fi.published as item_published
            FROM feed_subscriptions fs
            JOIN feeds f ON fs.feed_id = f.id
            LEFT JOIN feed_items fi ON fi.id = (
                SELECT id FROM feed_items WHERE feed_id = f.id ORDER BY published DESC LIMIT 1
            )
            WHERE fs.subscriber_id = ?
            ORDER BY f.name
            LIMIT ? OFFSET ?
            "#
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

// ============================================================================
// ServerSettingsTable
// ============================================================================

impl_table!(
    ServerSettingsTable,
    ServerSettingsModel,
    "server_settings",
    guild_id,
    u64,
    i64,
    r#"CREATE TABLE IF NOT EXISTS server_settings (
        guild_id INTEGER PRIMARY KEY,
        settings TEXT NOT NULL
    );"#,
    "guild_id, settings",
    "?, ?",
    "guild_id = ?, settings = ?",
    [guild_id, settings]
);

// ============================================================================
// VoiceSessionsTable
// ============================================================================

impl_table!(
    VoiceSessionsTable,
    VoiceSessionsModel,
    "voice_sessions",
    id,
    i32,
    i32,
    r#"CREATE TABLE IF NOT EXISTS voice_sessions (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        user_id INTEGER NOT NULL,
        guild_id INTEGER NOT NULL,
        channel_id INTEGER NOT NULL,
        join_time TIMESTAMP NOT NULL,
        leave_time TIMESTAMP NOT NULL,
        UNIQUE(user_id, channel_id, join_time)
    );"#,
    "user_id, guild_id, channel_id, join_time, leave_time",
    "?, ?, ?, ?, ?",
    "user_id = ?, guild_id = ?, channel_id = ?, join_time = ?, leave_time = ?",
    [user_id, guild_id, channel_id, join_time, leave_time]
);

impl VoiceSessionsTable {
    pub async fn get_leaderboard_opt(
        &self,
        opts: &VoiceLeaderboardOpt,
    ) -> Result<Vec<VoiceLeaderboardEntry>, DatabaseError> {
        let limit = opts.limit.unwrap_or(10) as i64;
        let offset = opts.offset.unwrap_or(0) as i64;

        // Build query dynamically based on which filters are present
        let mut query = String::from(
            r#"
            SELECT 
                user_id, 
                SUM(
                    CASE 
                        WHEN leave_time = join_time 
                        THEN strftime('%s', 'now') - strftime('%s', join_time)
                        ELSE strftime('%s', leave_time) - strftime('%s', join_time)
                    END
                ) as total_duration
            FROM voice_sessions
            WHERE guild_id = ?
            "#,
        );

        // Add time range filters if provided
        if opts.since.is_some() {
            query.push_str(" AND join_time >= ?");
        }
        if opts.until.is_some() {
            query.push_str(" AND join_time <= ?");
        }

        query.push_str(" GROUP BY user_id ORDER BY total_duration DESC LIMIT ? OFFSET ?");

        // Build the query
        let mut q = sqlx::query_as::<_, VoiceLeaderboardEntry>(&query).bind(opts.guild_id as i64);

        // Bind time filters
        if let Some(since) = opts.since {
            q = q.bind(since);
        }
        if let Some(until) = opts.until {
            q = q.bind(until);
        }

        // Bind limit and offset
        q = q.bind(limit).bind(offset);

        Ok(q.fetch_all(&self.base.pool).await?)
    }

    pub async fn get_leaderboard(
        &self,
        guild_id: u64,
        limit: u32,
    ) -> Result<Vec<VoiceLeaderboardEntry>, DatabaseError> {
        let opts = VoiceLeaderboardOptBuilder::default()
            .guild_id(guild_id)
            .limit(Some(limit))
            .build()
            .map_err(AppError::from)?;
        self.get_leaderboard_opt(&opts).await
    }

    pub async fn get_leaderboard_with_offset(
        &self,
        guild_id: u64,
        offset: u32,
        limit: u32,
    ) -> Result<Vec<VoiceLeaderboardEntry>, DatabaseError> {
        let opts = VoiceLeaderboardOptBuilder::default()
            .guild_id(guild_id)
            .offset(Some(offset))
            .limit(Some(limit))
            .build()
            .map_err(AppError::from)?;
        self.get_leaderboard_opt(&opts).await
    }

    /// Update leave_time for a specific session (user, channel, join_time combination)
    pub async fn update_leave_time(
        &self,
        user_id: u64,
        channel_id: u64,
        join_time: &chrono::DateTime<chrono::Utc>,
        leave_time: &chrono::DateTime<chrono::Utc>,
    ) -> Result<(), DatabaseError> {
        sqlx::query(
            r#"
            UPDATE voice_sessions 
            SET leave_time = ?
            WHERE user_id = ? AND channel_id = ? AND join_time = ?
            "#,
        )
        .bind(leave_time)
        .bind(user_id as i64)
        .bind(channel_id as i64)
        .bind(join_time)
        .execute(&self.base.pool)
        .await?;
        Ok(())
    }

    /// Find all active sessions (where leave_time equals join_time)
    pub async fn find_active_sessions(&self) -> Result<Vec<VoiceSessionsModel>, DatabaseError> {
        Ok(sqlx::query_as::<_, VoiceSessionsModel>(
            r#"
            SELECT * FROM voice_sessions
            WHERE leave_time = join_time
            "#,
        )
        .fetch_all(&self.base.pool)
        .await?)
    }

    /// Get daily voice activity for a specific user in a guild.
    /// Returns daily totals within the specified time range.
    pub async fn get_user_daily_activity(
        &self,
        user_id: u64,
        guild_id: u64,
        since: &chrono::DateTime<chrono::Utc>,
        until: &chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<crate::model::VoiceDailyActivity>, DatabaseError> {
        Ok(sqlx::query_as::<_, crate::model::VoiceDailyActivity>(
            r#"
            SELECT 
                date(join_time) as day,
                SUM(
                    CASE 
                        WHEN leave_time = join_time 
                        THEN strftime('%s', 'now') - strftime('%s', join_time)
                        ELSE strftime('%s', leave_time) - strftime('%s', join_time)
                    END
                ) as total_seconds
            FROM voice_sessions
            WHERE user_id = ? AND guild_id = ? AND join_time >= ? AND join_time <= ?
            GROUP BY date(join_time)
            ORDER BY day
            "#,
        )
        .bind(user_id as i64)
        .bind(guild_id as i64)
        .bind(since)
        .bind(until)
        .fetch_all(&self.base.pool)
        .await?)
    }

    /// Get guild-wide daily statistics: average time per active user.
    pub async fn get_guild_daily_average_time(
        &self,
        guild_id: u64,
        since: &chrono::DateTime<chrono::Utc>,
        until: &chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<crate::model::GuildDailyStats>, DatabaseError> {
        Ok(sqlx::query_as::<_, crate::model::GuildDailyStats>(
            r#"
            SELECT 
                day,
                CAST(AVG(user_daily_total) AS INTEGER) as value
            FROM (
                SELECT 
                    user_id,
                    date(join_time) as day,
                    SUM(
                        CASE 
                            WHEN leave_time = join_time 
                            THEN strftime('%s', 'now') - strftime('%s', join_time)
                            ELSE strftime('%s', leave_time) - strftime('%s', join_time)
                        END
                    ) as user_daily_total
                FROM voice_sessions
                WHERE guild_id = ? AND join_time >= ? AND join_time <= ?
                GROUP BY user_id, date(join_time)
            ) user_totals
            GROUP BY day
            ORDER BY day
            "#,
        )
        .bind(guild_id as i64)
        .bind(since)
        .bind(until)
        .fetch_all(&self.base.pool)
        .await?)
    }

    /// Get guild-wide daily statistics: count of unique active users.
    pub async fn get_guild_daily_user_count(
        &self,
        guild_id: u64,
        since: &chrono::DateTime<chrono::Utc>,
        until: &chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<crate::model::GuildDailyStats>, DatabaseError> {
        Ok(sqlx::query_as::<_, crate::model::GuildDailyStats>(
            r#"
            SELECT 
                date(join_time) as day,
                COUNT(DISTINCT user_id) as value
            FROM voice_sessions
            WHERE guild_id = ? AND join_time >= ? AND join_time <= ?
            GROUP BY date(join_time)
            ORDER BY day
            "#,
        )
        .bind(guild_id as i64)
        .bind(since)
        .bind(until)
        .fetch_all(&self.base.pool)
        .await?)
    }
}

impl From<crate::model::VoiceLeaderboardOptBuilderError> for AppError {
    fn from(value: crate::model::VoiceLeaderboardOptBuilderError) -> Self {
        AppError::internal_with_ref(value)
    }
}

// ============================================================================
// BotMetaTable
// ============================================================================

impl_table!(
    BotMetaTable,
    BotMetaModel,
    "bot_meta",
    key,
    String,
    String,
    r#"CREATE TABLE IF NOT EXISTS bot_meta (
        key TEXT PRIMARY KEY,
        value TEXT NOT NULL
    )"#,
    "key, value",
    "?, ?",
    "key = ?, value = ?",
    [key, value]
);

impl BotMetaTable {
    /// Check if the bot_meta table exists
    pub async fn table_exists(&self) -> bool {
        sqlx::query("SELECT name FROM sqlite_master WHERE type='table' AND name='bot_meta'")
            .fetch_optional(&self.base.pool)
            .await
            .map(|opt| opt.is_some())
            .unwrap_or(false)
    }
}
