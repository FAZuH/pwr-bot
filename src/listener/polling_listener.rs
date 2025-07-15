use async_trait::async_trait;
use sqlx::{SqlitePool, sqlite::SqliteRow, Row};
use tokio::sync::RwLock;
use std::sync::Arc;
use std::collections::HashMap;
use std::time::Duration;
use crate::action::action::Action;
use crate::config::Config;
use crate::listener::listener::Listener;
use crate::source::source::UpdateSource;

pub struct PollingListener {
    db: SqlitePool,
    sources: HashMap<String, Arc<dyn UpdateSource>>,
    subscribers: Arc<RwLock<HashMap<(String, String), Vec<Box<dyn Action>>>>>,
    running: bool,
    interval: Duration
}

impl PollingListener {
    pub async fn new(config: Config) -> anyhow::Result<Self> {
        // Create db file if not exists
        if !std::fs::exists(&config.db_path).unwrap_or(false) {
            std::fs::write(&config.db_path, "")?;
        }
        // Get db pool
        let db_url = format!("sqlite://{}", config.db_path);
        let db = SqlitePool::connect(&db_url).await?;
        // Init db tables
        PollingListener::create_tables(&db).await?;
        Ok(Self {
            db,
            sources: HashMap::new(),
            subscribers: Arc::new(RwLock::new(HashMap::new())),
            running: false,
            interval: Duration::from_secs(config.poll_interval)
        })
    }

    pub fn register_source(&mut self, series_type: String, source: Arc<dyn UpdateSource>) {
        self.sources.insert(series_type, source);
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
        let interval_duration = self.interval.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(interval_duration);
            loop {
                interval.tick().await;
                if let Err(e) = Self::check_updates(db.clone(), sources.clone(), actions.clone()).await {
                    eprintln!("Error checking updates: {}", e);
                }
            }
        });
    }

    async fn check_updates(
        db: SqlitePool,
        sources: HashMap<String, Arc<dyn UpdateSource>>,
        actions: Arc<RwLock<HashMap<(String, String), Vec<Box<dyn Action>>>>>,
    ) -> anyhow::Result<()> {
        let subscriptions: Vec<SqliteRow> =
            sqlx::query("SELECT DISTINCT user_id, series_id, series_type FROM subscriptions")
                .fetch_all(&db)
                .await?;

        for sub in subscriptions {
            let user_id: String = sub.get("user_id");
            let series_id: String = sub.get("series_id");
            let series_type: String = sub.get("series_type");

            let source = sources
                .get(&series_type)
                .ok_or_else(|| anyhow::anyhow!("No source for {}", series_type))?;

            if let Some(event) = source.check_update(&series_id).await? {
                let last_update: Option<SqliteRow> = sqlx::query(
                    "SELECT last_chapter_id FROM last_updates WHERE series_id = ?",
                )
                .bind(&series_id)
                .fetch_optional(&db)
                .await?;

                let last_chapter_id = last_update.map(|row| row.get("last_chapter_id"));

                if last_chapter_id.as_ref() != Some(&event.content_id) {
                    let read_actions = actions.read().await;
                    if let Some(action_list) = read_actions.get(&(user_id.clone(), series_id.clone())) {
                        for action in action_list {
                            action.run(&event).await?;
                        }
                    }

                    sqlx::query("INSERT OR REPLACE INTO last_updates (series_id, series_type, last_chapter_id, last_updated) VALUES (?, ?, ?, ?)")
                        .bind(&series_id)
                        .bind(&series_type)
                        .bind(&event.content_id)
                        .bind(chrono::Utc::now().to_rfc3339())
                        .execute(&db)
                        .await?;
                }
            }
        }

        Ok(())
    }
}

#[async_trait]
impl Listener for PollingListener {
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

    fn start(&mut self) -> anyhow::Result<()> {
        if !self.running {
            self.running = true;
            self.spawn_check_loop();
        }
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        self.running = false;
        Ok(())
    }
}
