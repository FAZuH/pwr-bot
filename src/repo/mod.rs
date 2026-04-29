//! Data repository module.

pub mod error;
pub mod schema;
pub mod table;
pub mod traits;

use diesel::Connection;
use diesel_async::AsyncPgConnection;
use diesel_async::pooled_connection::AsyncDieselConnectionManager;
use diesel_async::pooled_connection::deadpool::Object;
use diesel_async::pooled_connection::deadpool::Pool;
use diesel_migrations::EmbeddedMigrations;
use diesel_migrations::MigrationHarness;
use diesel_migrations::embed_migrations;
use log::info;
use tokio::task;

use crate::repo::table::*;
use crate::repo::traits::*;

pub type DbPool = Pool<AsyncPgConnection>;
pub type DbConn = Object<AsyncPgConnection>;

const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");

/// Main database struct containing all table handlers.
pub struct Repository {
    pub feed: FeedTable,
    pub feed_item: FeedItemTable,
    pub subscriber: SubscriberTable,
    pub feed_subscription: FeedSubscriptionTable,
    pub server_settings: ServerSettingsTable,
    pub voice_sessions: VoiceSessionsTable,
    pub bot_meta: BotMetaTable,

    pool: DbPool,
    db_url: String,
}

impl Repository {
    /// Creates a new database connection and initializes table handlers.
    pub async fn new(db_url: impl Into<String>) -> anyhow::Result<Self> {
        let db_url = db_url.into();
        info!("connecting to db");
        let conf = AsyncDieselConnectionManager::new(db_url.clone());
        let pool: DbPool = Pool::builder(conf).max_size(5).build()?;
        info!("connected to db");

        Ok(Self {
            feed: FeedTable::new(pool.clone()),
            feed_item: FeedItemTable::new(pool.clone()),
            subscriber: SubscriberTable::new(pool.clone()),
            feed_subscription: FeedSubscriptionTable::new(pool.clone()),
            server_settings: ServerSettingsTable::new(pool.clone()),
            voice_sessions: VoiceSessionsTable::new(pool.clone()),
            bot_meta: BotMetaTable::new(pool.clone()),
            pool,
            db_url,
        })
    }

    /// Access the underlying connection pool.
    pub fn pool(&self) -> &DbPool {
        &self.pool
    }

    /// Runs database migrations from the migrations directory.
    pub async fn run_migrations(&self) -> anyhow::Result<()> {
        let db_url = self.db_url.clone();
        task::spawn_blocking(move || {
            let mut conn =
                diesel::PgConnection::establish(&db_url).expect("failed to connect for migrations");
            conn.run_pending_migrations(MIGRATIONS)
                .expect("failed to run migrations");
        })
        .await?;
        Ok(())
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

    /// Deletes all data from all tables and resets sequences. Use with caution!
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
