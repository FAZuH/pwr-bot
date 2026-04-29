//! Database module with SQLite storage and Diesel.

use deadpool::managed::Metrics;
use deadpool::managed::Pool;
use deadpool::managed::RecycleResult;
use diesel::SqliteConnection;
use diesel_async::RunQueryDsl;
use diesel_async::pooled_connection::AsyncDieselConnectionManager;
use diesel_async::pooled_connection::PoolError as DieselPoolError;
use diesel_async::sync_connection_wrapper::SyncConnectionWrapper;
use log::debug;
use log::info;
use tokio::task;

use crate::repo::table::*;
use crate::repo::traits::*;

pub mod error;
pub mod schema;
pub mod table;
pub mod traits;

/// Custom deadpool manager that sets `busy_timeout` on every new SQLite connection.
pub struct SqliteConnectionManager {
    inner: AsyncDieselConnectionManager<SyncConnectionWrapper<SqliteConnection>>,
    busy_timeout_ms: i32,
}

impl SqliteConnectionManager {
    pub fn new(database_url: impl Into<String>, busy_timeout_ms: i32) -> Self {
        Self {
            inner: AsyncDieselConnectionManager::new(database_url.into()),
            busy_timeout_ms,
        }
    }
}

impl deadpool::managed::Manager for SqliteConnectionManager {
    type Type = SyncConnectionWrapper<SqliteConnection>;
    type Error = DieselPoolError;

    async fn create(&self) -> Result<Self::Type, Self::Error> {
        let mut conn = self.inner.create().await?;
        diesel::sql_query(format!("PRAGMA busy_timeout = {}", self.busy_timeout_ms))
            .execute(&mut conn)
            .await
            .map_err(DieselPoolError::QueryError)?;
        Ok(conn)
    }

    async fn recycle(
        &self,
        conn: &mut Self::Type,
        metrics: &Metrics,
    ) -> RecycleResult<Self::Error> {
        self.inner.recycle(conn, metrics).await
    }
}

pub type DbPool = Pool<SqliteConnectionManager>;

/// Main database struct containing all table handlers.
pub struct Repository {
    pool: DbPool,
    db_path: String,
    pub feed: Box<dyn FeedRepository>,
    pub feed_item: Box<dyn FeedItemRepository>,
    pub subscriber: Box<dyn SubscriberRepository>,
    pub feed_subscription: Box<dyn FeedSubscriptionRepository>,
    pub server_settings: Box<dyn ServerSettingsRepository>,
    pub voice_sessions: Box<dyn VoiceSessionsRepository>,
    pub bot_meta: Box<dyn BotMetaRepository>,
}

impl Repository {
    /// Creates a new database connection and initializes table handlers.
    pub async fn new(db_url: &str, db_path: &str) -> anyhow::Result<Self> {
        let path = std::path::Path::new(db_path);
        if !path.exists() {
            debug!("Database path {db_path} does not exist. Creating...");
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(path, "")?;
            info!("Created {db_path}");
        }

        debug!("Connecting to db...");
        let manager = SqliteConnectionManager::new(db_url, 5000);
        let pool = Pool::builder(manager).max_size(5).build()?;
        log::log!(log::Level::Info, "Connected to db.");

        // Enable WAL mode for better concurrent read/write performance.
        let mut conn = pool.get().await?;
        diesel::sql_query("PRAGMA journal_mode = WAL")
            .execute(&mut conn)
            .await?;
        drop(conn);

        let feed = Box::new(FeedTable::new(pool.clone()));
        let feed_item = Box::new(FeedItemTable::new(pool.clone()));
        let subscriber = Box::new(SubscriberTable::new(pool.clone()));
        let feed_subscription = Box::new(FeedSubscriptionTable::new(pool.clone()));
        let server_settings = Box::new(ServerSettingsTable::new(pool.clone()));
        let voice_sessions = Box::new(VoiceSessionsTable::new(pool.clone()));
        let bot_meta = Box::new(BotMetaTable::new(pool.clone()));

        Ok(Self {
            pool,
            db_path: db_path.to_string(),
            feed,
            feed_item,
            subscriber,
            feed_subscription,
            server_settings,
            voice_sessions,
            bot_meta,
        })
    }

    /// Runs database migrations from the migrations directory.
    pub async fn run_migrations(&self) -> anyhow::Result<()> {
        let db_path = self.db_path.clone();
        task::spawn_blocking(move || {
            use diesel::Connection;
            use diesel::SqliteConnection;
            use diesel_migrations::MigrationHarness;
            use diesel_migrations::embed_migrations;

            const MIGRATIONS: diesel_migrations::EmbeddedMigrations =
                embed_migrations!("migrations");
            let mut conn = SqliteConnection::establish(&db_path)?;
            conn.run_pending_migrations(MIGRATIONS)
                .map_err(|e| anyhow::anyhow!(e))?;
            Ok::<(), anyhow::Error>(())
        })
        .await??;
        Ok(())
    }

    /// Access the underlying connection pool.
    pub fn pool(&self) -> &DbPool {
        &self.pool
    }

    /// Drops all tables. Use with caution!
    pub async fn drop_all_tables(&self) -> anyhow::Result<()> {
        self.feed.drop_table().await?;
        self.feed_item.drop_table().await?;
        self.subscriber.drop_table().await?;
        self.feed_subscription.drop_table().await?;
        self.server_settings.drop_table().await?;
        self.voice_sessions.drop_table().await?;
        self.bot_meta.drop_table().await?;
        Ok(())
    }

    /// Deletes all data from all tables. Use with caution!
    pub async fn delete_all_tables(&self) -> anyhow::Result<()> {
        self.feed.delete_all().await?;
        self.feed_item.delete_all().await?;
        self.subscriber.delete_all().await?;
        self.feed_subscription.delete_all().await?;
        self.server_settings.delete_all().await?;
        self.voice_sessions.delete_all().await?;
        self.bot_meta.delete_all().await?;
        Ok(())
    }
}
