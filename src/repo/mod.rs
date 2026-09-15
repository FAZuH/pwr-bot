//! Data repository module.

pub mod error;
pub mod postgres;
pub mod schema;
pub mod traits;

use anyhow::Context;
use diesel::Connection;
use diesel_async::AsyncPgConnection;
use diesel_async::pooled_connection::AsyncDieselConnectionManager;
use diesel_async::pooled_connection::deadpool::Pool;
use diesel_migrations::EmbeddedMigrations;
use diesel_migrations::MigrationHarness;
use diesel_migrations::embed_migrations;
use log::info;
use tokio::task;

use crate::repo::postgres::*;
use crate::repo::traits::*;

pub type DbPool = Pool<AsyncPgConnection>;

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
    pub plugin_kv: PgPluginKvRepo,
    pub guild_plugins: PgGuildPluginRepo,

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
            plugin_kv: PgPluginKvRepo::new(pool.clone()),
            guild_plugins: PgGuildPluginRepo::new(pool.clone()),
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
            let mut conn = diesel::PgConnection::establish(&db_url)
                .context("connecting to the database for migrations")?;
            conn.run_pending_migrations(MIGRATIONS)
                .map_err(anyhow::Error::from_boxed)
                .context("running pending database migrations")?;
            Ok(())
        })
        .await?
    }

    pub async fn delete_all_tables(&self) -> anyhow::Result<()> {
        self.feed.delete_all().await?;
        self.feed_item.delete_all().await?;
        self.subscriber.delete_all().await?;
        self.feed_subscription.delete_all().await?;
        self.server_settings.delete_all().await?;
        self.voice_sessions.delete_all().await?;
        self.bot_meta.delete_all().await?;
        self.plugin_kv.delete_all().await?;
        self.guild_plugins.delete_all().await?;
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

    fn plugin_kv(&self) -> Box<dyn PluginKvRepository + Send + Sync> {
        Box::new(self.plugin_kv.clone())
    }

    fn guild_plugins(&self) -> Box<dyn GuildPluginRepository + Send + Sync> {
        Box::new(self.guild_plugins.clone())
    }
}
