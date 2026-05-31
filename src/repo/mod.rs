//! Data repository module.

pub mod error;
pub mod postgres;
pub mod schema;
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

use crate::repo::postgres::*;
use crate::repo::traits::*;

pub type DbPool = Pool<AsyncPgConnection>;
pub type DbConn = Object<AsyncPgConnection>;

const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");

/// PostgreSQL factory providing access to individual repository handles.
///
/// Stores each concrete `Pg*Repo` as a `Box<dyn>`-compatible field. The `Repos`
/// factory trait methods clone the inner handle and return a `Box<dyn Repo>`.
/// Call factory methods at service construction time, not per-operation.
pub struct PgRepos {
    pub feed: PgFeedRepo,
    pub feed_item: PgFeedItemRepo,
    pub subscriber: PgSubscriberRepo,
    pub feed_subscription: PgFeedSubscriptionRepo,
    pub server_settings: PgServerSettingsRepo,
    pub voice_sessions: PgVoiceSessionsRepo,
    pub bot_meta: PgBotMetaRepo,

    pool: DbPool,
    db_url: String,
}

impl PgRepos {
    pub async fn new(db_url: impl Into<String>) -> anyhow::Result<Self> {
        let db_url = db_url.into();
        info!("connecting to db");
        let conf = AsyncDieselConnectionManager::new(db_url.clone());
        let pool: DbPool = Pool::builder(conf).max_size(5).build()?;
        info!("connected to db");

        Ok(Self {
            feed: PgFeedRepo::new(pool.clone()),
            feed_item: PgFeedItemRepo::new(pool.clone()),
            subscriber: PgSubscriberRepo::new(pool.clone()),
            feed_subscription: PgFeedSubscriptionRepo::new(pool.clone()),
            server_settings: PgServerSettingsRepo::new(pool.clone()),
            voice_sessions: PgVoiceSessionsRepo::new(pool.clone()),
            bot_meta: PgBotMetaRepo::new(pool.clone()),
            pool,
            db_url,
        })
    }

    pub fn pool(&self) -> &DbPool {
        &self.pool
    }

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

impl Repos for PgRepos {
    fn feed(&self) -> Box<dyn FeedRepository + Send + Sync> {
        Box::new(self.feed.clone())
    }

    fn feed_item(&self) -> Box<dyn FeedItemRepository + Send + Sync> {
        Box::new(self.feed_item.clone())
    }

    fn subscriber(&self) -> Box<dyn SubscriberRepository + Send + Sync> {
        Box::new(self.subscriber.clone())
    }

    fn feed_subscription(&self) -> Box<dyn FeedSubscriptionRepository + Send + Sync> {
        Box::new(self.feed_subscription.clone())
    }

    fn server_settings(&self) -> Box<dyn ServerSettingsRepository + Send + Sync> {
        Box::new(self.server_settings.clone())
    }

    fn voice_sessions(&self) -> Box<dyn VoiceSessionsRepository + Send + Sync> {
        Box::new(self.voice_sessions.clone())
    }

    fn bot_meta(&self) -> Box<dyn BotMetaRepository + Send + Sync> {
        Box::new(self.bot_meta.clone())
    }
}
