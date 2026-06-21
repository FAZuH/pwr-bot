pub mod update;

use deadpool_postgres::ManagerConfig;
use deadpool_postgres::Pool;
use deadpool_postgres::RecyclingMethod;
use pwr_bot_plugin_util::query_json;
use pwr_bot_sdk::*;
use tokio::sync::Mutex;

pwr_bot_sdk::export_plugin!(WelcomePlugin, WelcomePlugin::new());

pub struct WelcomePlugin {
    pool: Mutex<Option<Pool>>,
}

impl WelcomePlugin {
    pub fn new() -> Self {
        Self {
            pool: Mutex::new(None),
        }
    }
}

impl Default for WelcomePlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl BotPlugin for WelcomePlugin {
    fn name(&self) -> &'static str {
        "welcome"
    }

    fn description(&self) -> &'static str {
        "Welcome message management"
    }

    fn version(&self) -> &'static str {
        "0.1.0"
    }

    fn commands(&self) -> Vec<CommandSpec> {
        vec![
            CommandSpec::new("welcome", "Manage welcome message settings"),
            CommandSpec::new("welcome settings", "Configure welcome settings"),
        ]
    }

    fn settings_panels(&self) -> Vec<SettingsPanelSpec> {
        vec![SettingsPanelSpec::new("welcome", "Welcome")]
    }

    fn test_steps(&self) -> Vec<TestStepSpec> {
        vec![TestStepSpec::new(
            "welcome settings",
            "Welcome settings",
            "welcome",
            serde_json::json!({}),
        )]
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
        Ok(())
    }

    async fn invoke(
        &self,
        command: &str,
        _args: serde_json::Value,
        host: &PluginHost,
    ) -> Result<ResponsePayload, String> {
        match command {
            "welcome" | "welcome settings" => self.cmd_show(host).await,
            _ => Err(format!("Unknown command: {command}")),
        }
    }
}

impl WelcomePlugin {
    async fn cmd_show(&self, host: &PluginHost) -> Result<ResponsePayload, String> {
        unsafe { host.defer().map_err(|e| e.to_string())? };

        let guild_id = unsafe { host.guild_id() };
        if guild_id == 0 {
            return Ok(ResponsePayload::text_ephemeral(
                "This command can only be used in a server.",
            ));
        }

        let pool = self
            .pool
            .lock()
            .await
            .as_ref()
            .ok_or("Database pool not initialized")?
            .clone();

        let result = query_json(
            &pool,
            "SELECT settings FROM server_settings WHERE guild_id = $1",
            &[&(guild_id as i64)],
        )
        .await?;

        let welcome = result
            .as_array()
            .and_then(|arr| arr.first())
            .and_then(|r| r.get("settings"))
            .and_then(|s| s.get("welcome"));

        let enabled = welcome
            .and_then(|w| w.get("enabled"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let channel = welcome
            .and_then(|w| w.get("channel_id"))
            .and_then(|v| v.as_str())
            .unwrap_or("Not set");
        let template = welcome
            .and_then(|w| w.get("template_id"))
            .and_then(|v| v.as_str())
            .unwrap_or("1");
        let color = welcome
            .and_then(|w| w.get("primary_color"))
            .and_then(|v| v.as_str())
            .unwrap_or("#5865F2");
        let msg_count = welcome
            .and_then(|w| w.get("messages"))
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);

        let content = format!(
            "## Welcome Settings\n\
             - **Enabled:** {}\n\
             - **Channel:** {}\n\
             - **Template:** {}\n\
             - **Color:** {}\n\
             - **Messages:** {}",
            if enabled { "✅ Yes" } else { "❌ No" },
            channel,
            template,
            color,
            msg_count,
        );

        Ok(ResponsePayload::text(content))
    }
}
