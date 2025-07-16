use sqlx::{SqlitePool, sqlite::SqliteRow, Row};
use tokio::sync::RwLock;
use std::sync::Arc;
use std::collections::{HashMap, HashSet};
use std::time::Duration;
use crate::action::action::Action;
use crate::config::Config;
use crate::source::source::Source;

pub struct UpdatePublisher {
    db: SqlitePool,
    sources: HashSet<Source>,
    subscribers: Arc<RwLock<HashMap<(String, String), HashSet<Box<dyn Action>>>>>,
    running: bool,
    interval: Duration
}

impl UpdatePublisher {
    pub async fn new(config: Config) -> anyhow::Result<Self> {
        // Create db file if not exists
        if !std::fs::exists(&config.db_path).unwrap_or(false) {
            std::fs::write(&config.db_path, "")?;
        }
        // Get db pool
        let db = SqlitePool::connect(&format!("sqlite://{}", config.db_path)).await?;
        // Init db tables
        UpdatePublisher::create_tables(&db).await?;
        Ok(Self {
            db,
            sources: HashSet::<Source>::new(),
            subscribers: Arc::new(RwLock::new(HashMap::new())),
            running: false,
            interval: Duration::from_secs(config.poll_interval)
        })
    }

    pub fn register_source(&mut self, source: Source) {
        self.sources.insert(source);
    }

    pub fn start(&mut self) -> anyhow::Result<()> {
        if !self.running {
            self.running = true;
            self.spawn_check_loop();
        }
        Ok(())
    }

    pub fn stop(&mut self) -> anyhow::Result<()> {
        self.running = false;
        Ok(())
    }

    async fn create_tables(db: &SqlitePool) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS subscriptions (
                user_id TEXT,
                series_id TEXT,
                series_type TEXT,
                action_type TEXT,
                action_data TEXT,
                PRIMARY KEY (user_id, series_id, action_type)
            )
            "#,
        )
        .execute(db)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS last_updates (
                series_id TEXT PRIMARY KEY,
                series_type TEXT,
                last_chapter_id TEXT,
                last_updated TIMESTAMP
            )
            "#,
        )
        .execute(db)
        .await?;

        Ok(())
    }

    fn spawn_check_loop(&self) {
        let db = self.db.clone();
        let sources = self.sources.clone();
        let actions = self.subscribers.clone();
        let interval_duration = self.interval;  // Duration implements Copy, no need to clone

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(interval_duration);
            loop {
                interval.tick().await;
                // Pass references instead of cloning every iteration
                if let Err(e) = Self::check_updates(&db, &sources, &actions).await {
                    eprintln!("Error checking updates: {}", e);
                }
            }
        });
    }

    async fn check_updates(
        db: &SqlitePool,
        sources: &HashSet<Source>,
        actions: &Arc<RwLock<HashMap<(String, String), HashSet<Box<dyn Action>>>>>,
    ) -> anyhow::Result<()> {
        let subscriptions: Vec<SqliteRow> =
            sqlx::query("SELECT DISTINCT user_id, series_id, series_type FROM subscriptions")
                .fetch_all(db)
                .await?;

        for sub in subscriptions {
            let user_id: String = sub.get("user_id");
            let series_id: String = sub.get("series_id");
            let series_type: String = sub.get("series_type");

            let source = series_type
                .parse::<Source>()
                .ok()
                .and_then(|s| sources.get(&s))
                .ok_or_else(|| anyhow::anyhow!("No source for {}", series_type))?;

            let latest_id: Option<String> = match source {
                Source::Anime(anime) => anime.get_latest(series_id.as_str()).await?.map(|series| series.series_id),
                Source::Manga(manga) => manga.get_latest(series_id.as_str()).await?.map(|series| series.series_id),
            };
            let Some(latest_id) = latest_id else { continue; };
            // latest_id is not None

            let last_update: Option<SqliteRow> = sqlx::query(
                "SELECT last_chapter_id FROM last_updates WHERE series_id = ?",
            )
            .bind(&series_id)
            .fetch_optional(db)
            .await?;

            // Possibilities:
            // 1. last_chapter_id is None. update db & publish event
            // 2. last_chapter_id != latest_id. update db & publish event
            // 3. last_chapter_id == latest_id. continue

            let last_chapter_id: Option<String> = last_update.map(|row| row.get("last_chapter_id"));

            sqlx::query("INSERT OR REPLACE INTO last_updates (series_id, series_type, last_chapter_id, last_updated) VALUES (?, ?, ?, ?)")
                .bind(&series_id)
                .bind(&series_type)
                .bind(&latest_id)
                .bind(chrono::Utc::now().to_rfc3339())
                .execute(db)
                .await?;
        }

        Ok(())
    }

    async fn subscribe(&mut self, user_id: String, series_id: String, series_type: String, webhook_url: String) -> anyhow::Result<()> {
        let action_type = std::any::type_name::<Box<dyn Action>>().to_string();
        sqlx::query("INSERT OR REPLACE INTO subscriptions VALUES (?, ?, ?, ?, ?)")
            .bind(&user_id)
            .bind(&series_id)
            .bind(&series_type)
            .bind(&action_type)
            .bind(&webhook_url)
            .execute(&self.db)
            .await?;
        Ok(())
    }

    async fn unsubscribe(&mut self, user_id: String, series_id: String) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM subscriptions WHERE user_id = ? AND series_id = ?")
            .bind(user_id)
            .bind(series_id)
            .execute(&self.db)
            .await?;
        Ok(())
    }

}
