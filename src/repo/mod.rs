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
use tokio::task;
use tracing::info;

use crate::repo::postgres::*;
use crate::repo::traits::*;

pub type DbPool = Pool<AsyncPgConnection>;
pub type DbConn = Object<AsyncPgConnection>;

const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");

pub struct PgRepos {
    pub server_settings: PgServerSettingsRepo,
    pub bot_meta: PgBotMetaRepo,
    pool: DbPool,
    db_url: String,
}

impl PgRepos {
    pub async fn new(db_url: impl Into<String>) -> anyhow::Result<Self> {
        let db_url = db_url.into();
        info!("connecting to database");
        let conf = AsyncDieselConnectionManager::new(db_url.clone());
        let pool: DbPool = Pool::builder(conf).max_size(5).build()?;
        info!("connected to database");

        Ok(Self {
            server_settings: PgServerSettingsRepo::new(pool.clone()),
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
}

impl Repos for PgRepos {
    fn server_settings(&self) -> Box<dyn ServerSettingsRepository + Send + Sync> {
        Box::new(self.server_settings.clone())
    }

    fn bot_meta(&self) -> Box<dyn BotMetaRepository + Send + Sync> {
        Box::new(self.bot_meta.clone())
    }
}
