use anyhow::Context;
use diesel::Connection;
use diesel_async::AsyncPgConnection;
use diesel_async::pooled_connection::AsyncDieselConnectionManager;
use diesel_async::pooled_connection::deadpool::Pool;
use diesel_migrations::EmbeddedMigrations;
use diesel_migrations::MigrationHarness;
use diesel_migrations::embed_migrations;

pub type DbPool = Pool<AsyncPgConnection>;

const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");

#[derive(Clone)]
pub struct Store {
    pool: DbPool,
    db_url: String,
}

impl Store {
    pub async fn connect(db_url: impl Into<String>) -> anyhow::Result<Self> {
        let db_url = db_url.into();
        let manager = AsyncDieselConnectionManager::new(db_url.clone());
        let pool = Pool::builder(manager).max_size(5).build()?;
        Ok(Self { pool, db_url })
    }

    pub fn pool(&self) -> &DbPool {
        &self.pool
    }

    pub async fn migrate(&self) -> anyhow::Result<Vec<String>> {
        let db_url = self.db_url.clone();
        tokio::task::spawn_blocking(move || {
            let mut connection = diesel::PgConnection::establish(&db_url)
                .context("connect to the database for feed migrations")?;
            connection
                .run_pending_migrations(MIGRATIONS)
                .map(|versions| {
                    versions
                        .into_iter()
                        .map(|version| version.to_string())
                        .collect()
                })
                .map_err(anyhow::Error::from_boxed)
                .context("run pending feed migrations")
        })
        .await?
    }
}
