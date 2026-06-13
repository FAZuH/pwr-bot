pub mod error;
pub mod platform;
mod publisher;
mod subscriber;
pub mod subscription;
pub mod update;

use std::sync::Arc;

use deadpool_postgres::ManagerConfig;
use deadpool_postgres::Pool;
use deadpool_postgres::RecyclingMethod;
use pwr_bot_sdk::*;
use tokio::sync::Mutex;

use crate::platform::Platforms;

pwr_bot_sdk::export_plugin!(FeedPlugin, FeedPlugin::new());

pub struct FeedPlugin {
    platforms: Mutex<Option<Arc<Platforms>>>,
    pool: Mutex<Option<Pool>>,
}

impl FeedPlugin {
    pub fn new() -> Self {
        Self {
            platforms: Mutex::new(None),
            pool: Mutex::new(None),
        }
    }

    async fn query_json(
        &self,
        sql: &str,
        params: &[&(dyn tokio_postgres::types::ToSql + Sync)],
    ) -> Result<serde_json::Value, String> {
        let pool = self
            .pool
            .lock()
            .await
            .as_ref()
            .ok_or("Database pool not initialized")?
            .clone();
        let client = pool.get().await.map_err(|e| e.to_string())?;
        let rows = client.query(sql, params).await.map_err(|e| e.to_string())?;
        let json_rows: Vec<serde_json::Value> = rows
            .iter()
            .map(|row| {
                let mut map = serde_json::Map::new();
                for (i, col) in row.columns().iter().enumerate() {
                    let name = col.name();
                    let value: serde_json::Value = row
                        .try_get::<_, serde_json::Value>(i)
                        .unwrap_or(serde_json::Value::Null);
                    map.insert(name.to_string(), value);
                }
                serde_json::Value::Object(map)
            })
            .collect();
        Ok(serde_json::Value::Array(json_rows))
    }

    async fn db_execute(
        &self,
        sql: &str,
        params: &[&(dyn tokio_postgres::types::ToSql + Sync)],
    ) -> Result<u64, String> {
        let pool = self
            .pool
            .lock()
            .await
            .as_ref()
            .ok_or("Database pool not initialized")?
            .clone();
        let client = pool.get().await.map_err(|e| e.to_string())?;
        client.execute(sql, params).await.map_err(|e| e.to_string())
    }
}

impl Default for FeedPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl BotPlugin for FeedPlugin {
    fn name(&self) -> &'static str {
        "feed"
    }

    fn description(&self) -> &'static str {
        "Feed subscription management"
    }

    fn version(&self) -> &'static str {
        "0.1.0"
    }

    fn commands(&self) -> Vec<CommandSpec> {
        vec![CommandSpec {
            name: "feed".into(),
            description: "Manage feed subscriptions".into(),
            args: vec![],
        }]
    }

    fn settings_panels(&self) -> Vec<SettingsPanelSpec> {
        vec![SettingsPanelSpec::new("feeds", "Feeds")]
    }

    fn tasks(&self) -> Vec<TaskSpec> {
        vec![TaskSpec::new("feed-publisher", 60, "__poll_feeds")]
    }

    fn test_steps(&self) -> Vec<TestStepSpec> {
        vec![TestStepSpec::new(
            "feed list",
            "Subscription list (empty)",
            "feed list",
            serde_json::json!({"action": "list"}),
        )]
    }

    fn event_handlers(&self) -> Vec<EventHandlerSpec> {
        vec![EventHandlerSpec::new("feed_update".to_string())]
    }

    async fn init(&self, _host: &PluginHost) -> Result<(), String> {
        let db_url = std::env::var("DB_URL")
            .map_err(|_| "DB_URL environment variable not set".to_string())?;
        let mut config = deadpool_postgres::Config::new();
        config.url = Some(db_url);
        config.manager = Some(ManagerConfig {
            recycling_method: RecyclingMethod::Fast,
        });
        let pool = config
            .create_pool(
                Some(deadpool_postgres::Runtime::Tokio1),
                tokio_postgres::NoTls,
            )
            .map_err(|e| format!("Failed to create database pool: {e}"))?;
        *self.pool.lock().await = Some(pool);

        let platforms = Arc::new(Platforms::new());
        *self.platforms.lock().await = Some(platforms);
        Ok(())
    }

    async fn on_event(
        &self,
        event_name: &str,
        payload: serde_json::Value,
        host: &PluginHost,
    ) -> Result<(), String> {
        match event_name {
            "feed_update" => {
                let pool = self
                    .pool
                    .lock()
                    .await
                    .as_ref()
                    .ok_or("Database pool not initialized")?
                    .clone();
                subscriber::handle_feed_update(&pool, host, payload).await
            }
            _ => Ok(()),
        }
    }

    async fn invoke(
        &self,
        command: &str,
        _args: serde_json::Value,
        host: &PluginHost,
    ) -> Result<ResponsePayload, String> {
        match command {
            "feed" | "feed list" => self.cmd_list(host).await,
            "feed settings" => self.cmd_settings(host).await,
            "__poll_feeds" => self.cmd_poll_feeds(host).await,
            _ => Err(format!("Unknown command: {command}")),
        }
    }
}

impl FeedPlugin {
    async fn cmd_settings(&self, host: &PluginHost) -> Result<ResponsePayload, String> {
        unsafe { host.defer().map_err(|e| e.to_string())? };

        let guild_id = unsafe { host.guild_id() };
        if guild_id == 0 {
            return Ok(ResponsePayload {
                content: Some("This command can only be used in a server.".into()),
                ephemeral: true,
                components_json: None,
                embed_json: None,
            });
        }

        let result = self
            .query_json(
                "SELECT settings FROM server_settings WHERE guild_id = $1",
                &[&(guild_id as i64)],
            )
            .await?;

        let feed = result
            .as_array()
            .and_then(|arr| arr.first())
            .and_then(|r| r.get("settings"))
            .and_then(|s| s.get("feeds"));

        let enabled = feed
            .and_then(|f| f.get("enabled"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let channel = feed
            .and_then(|f| f.get("channel_id"))
            .and_then(|v| v.as_str())
            .unwrap_or("Not set");
        let sub_role = feed
            .and_then(|f| f.get("subscribe_role_id"))
            .and_then(|v| v.as_str());
        let unsub_role = feed
            .and_then(|f| f.get("unsubscribe_role_id"))
            .and_then(|v| v.as_str());

        let content = format!(
            "## Feed Settings\n\
             - **Enabled:** {}\n\
             - **Notification Channel:** {}\n\
             - **Subscribe Role:** {}\n\
             - **Unsubscribe Role:** {}",
            if enabled { "✅ Yes" } else { "❌ No" },
            channel,
            sub_role
                .map(|r| format!("<@&{r}>"))
                .as_deref()
                .unwrap_or("None"),
            unsub_role
                .map(|r| format!("<@&{r}>"))
                .as_deref()
                .unwrap_or("None"),
        );

        Ok(ResponsePayload {
            content: Some(content),
            ephemeral: false,
            components_json: None,
            embed_json: None,
        })
    }

    async fn cmd_list(&self, host: &PluginHost) -> Result<ResponsePayload, String> {
        unsafe { host.defer().map_err(|e| e.to_string())? };

        let _guild_id = unsafe { host.guild_id() };
        let author_id = unsafe { host.author_id() };

        let result = self
            .query_json(
                "SELECT s.id, s.type_, s.target_id \
                 FROM subscribers s \
                 WHERE s.target_id = $1 AND s.type_ = 'dm'",
                &[&(author_id as i64)],
            )
            .await?;

        let rows = match result {
            serde_json::Value::Array(arr) => arr,
            _ => vec![],
        };

        if rows.is_empty() {
            return Ok(ResponsePayload {
                content: Some(
                    "You have no subscriptions. Use `/feed subscribe` to add some!".into(),
                ),
                ephemeral: false,
                components_json: None,
                embed_json: None,
            });
        }

        let subscriber_id = rows[0].get("id").and_then(|v| v.as_i64()).unwrap_or(0);

        let feed_result = self
            .query_json(
                "SELECT f.id, f.name, f.description, f.platform_id, f.source_id, \
                        f.items_id, f.source_url, f.cover_url, f.tags, \
                        fi.description as item_desc, fi.published as item_published \
                 FROM feed_subscriptions fs \
                 JOIN feeds f ON f.id = fs.feed_id \
                 LEFT JOIN feed_items fi ON fi.id = f.items_id \
                 WHERE fs.subscriber_id = $1 \
                 ORDER BY f.name",
                &[&subscriber_id],
            )
            .await?;

        let feeds = match feed_result {
            serde_json::Value::Array(arr) => arr,
            _ => vec![],
        };

        let mut lines: Vec<String> = Vec::new();
        lines.push("## Your Subscriptions".to_string());

        for (i, feed) in feeds.iter().enumerate() {
            let name = feed
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown");
            let source_url = feed
                .get("source_url")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let tags = feed.get("tags").and_then(|v| v.as_str()).unwrap_or("");
            let platform = feed
                .get("platform_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let item_desc = feed
                .get("item_desc")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty());

            let line = match item_desc {
                Some(latest) => {
                    format!(
                        "{}. **[{name}](<{source_url}>)** ({platform})\n   └ Latest: {latest}",
                        i + 1
                    )
                }
                None => {
                    format!("{}. **[{name}](<{source_url}>)** ({platform})", i + 1)
                }
            };
            lines.push(line);

            if !tags.is_empty() {
                lines.push(format!("   └ Tags: {tags}"));
            }
        }

        Ok(ResponsePayload {
            content: Some(lines.join("\n")),
            ephemeral: false,
            components_json: None,
            embed_json: None,
        })
    }

    async fn cmd_poll_feeds(&self, host: &PluginHost) -> Result<ResponsePayload, String> {
        let pool = self
            .pool
            .lock()
            .await
            .as_ref()
            .ok_or("Database pool not initialized")?
            .clone();
        let platforms = self.platforms.lock().await;
        let platforms = platforms.as_ref().ok_or("Platforms not initialized")?;
        publisher::poll_feeds(&pool, host, platforms).await?;
        Ok(ResponsePayload {
            content: None,
            ephemeral: false,
            components_json: None,
            embed_json: None,
        })
    }
}
