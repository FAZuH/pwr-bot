pub mod error;
pub mod postgres;
pub mod schema;
pub mod traits;

use diesel_async::AsyncMigrationHarness;
use diesel_async::AsyncPgConnection;
use diesel_async::pooled_connection::AsyncDieselConnectionManager;
use diesel_async::pooled_connection::deadpool::Object;
use diesel_async::pooled_connection::deadpool::Pool;
use diesel_migrations::EmbeddedMigrations;
use diesel_migrations::MigrationHarness;
use diesel_migrations::embed_migrations;
use percent_encoding::percent_decode_str;
use tracing::info;
use url::Url;

use crate::repo::postgres::*;
use crate::repo::traits::*;

pub type DbPool = Pool<AsyncPgConnection>;
pub type DbConn = Object<AsyncPgConnection>;

const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");

pub struct PgRepos {
    pub server_settings: PgServerSettingsRepo,
    pub bot_meta: PgBotMetaRepo,
    pool: DbPool,
}

/// Converts a `postgres://…` URI to libpq key=value format.
///
/// `PQconnectdb` does NOT percent-decode the password field, so we parse the
/// URI with the `url` crate, percent-decode each component, and emit
/// `key=value` pairs that libpq can parse unambiguously.
fn pg_connstr(uri: &str) -> String {
    let parsed = Url::parse(uri).expect("invalid database URL");
    let host = parsed.host_str().unwrap_or("localhost");
    let dbname = parsed.path().trim_start_matches('/');
    let user = percent_decode_str(parsed.username())
        .decode_utf8()
        .unwrap_or_default();
    let password = parsed
        .password()
        .map(|p| percent_decode_str(p).decode_utf8().unwrap_or_default())
        .unwrap_or_default();

    // Single-quote values that contain `=`, space, or `'` so libpq doesn't
    // misinterpret them as key=value separators.
    fn q(v: &str) -> String {
        if v.contains('=') || v.contains(' ') || v.contains('\'') {
            format!("'{}'", v.replace('\'', r"\'"))
        } else {
            v.to_string()
        }
    }

    let mut parts: Vec<String> = Vec::new();
    if !user.is_empty() {
        parts.push(format!("user={}", q(&user)));
    }
    if !password.is_empty() {
        parts.push(format!("password={}", q(&password)));
    }
    parts.push(format!("host={host}"));
    if let Some(port) = parsed.port() {
        parts.push(format!("port={port}"));
    }
    if !dbname.is_empty() {
        parts.push(format!("dbname={}", q(dbname)));
    }
    for (k, v) in parsed.query_pairs() {
        let k: &str = &k;
        let v: &str = &v;
        parts.push(format!("{k}={}", q(v)));
    }

    parts.join(" ")
}

impl PgRepos {
    pub async fn new(db_url: impl Into<String>) -> anyhow::Result<Self> {
        let db_url = db_url.into();
        info!("connecting to database");
        let connstr = pg_connstr(&db_url);
        let conf = AsyncDieselConnectionManager::new(connstr);
        let pool: DbPool = Pool::builder(conf).max_size(5).build()?;
        info!("connected to database");

        Ok(Self {
            server_settings: PgServerSettingsRepo::new(pool.clone()),
            bot_meta: PgBotMetaRepo::new(pool.clone()),
            pool,
        })
    }

    pub fn pool(&self) -> &DbPool {
        &self.pool
    }

    pub async fn run_migrations(&self) -> anyhow::Result<()> {
        let conn = self.pool.get().await?;
        info!("running database migrations");
        let mut harness = AsyncMigrationHarness::new(conn);
        harness
            .run_pending_migrations(MIGRATIONS)
            .expect("failed to run migrations");
        info!("database migrations complete");
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
