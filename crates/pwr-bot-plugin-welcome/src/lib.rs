use pwr_bot_sdk::*;
use serde::Deserialize;

pub struct WelcomePlugin;

#[derive(Default, Deserialize)]
struct PluginWelcomeSettings {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub channel_id: Option<String>,
    #[serde(default)]
    pub primary_color: Option<String>,
    #[serde(default)]
    pub template_id: Option<String>,
    #[serde(default)]
    pub messages: Option<Vec<String>>,
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
        vec![CommandSpec {
            name: "welcome".into(),
            description: "Manage welcome message settings".into(),
            args: vec![],
        }]
    }

    async fn invoke(
        &self,
        command: &str,
        _args: serde_json::Value,
        host: &PluginHost,
    ) -> Result<ResponsePayload, String> {
        match command {
            "welcome" => self.cmd_show(host).await,
            _ => Err(format!("Unknown command: {command}")),
        }
    }
}

impl WelcomePlugin {
    async fn cmd_show(&self, host: &PluginHost) -> Result<ResponsePayload, String> {
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

        let result = unsafe {
            host.query_db(
                "SELECT settings FROM server_settings WHERE guild_id = $1",
                &[DbValue::I64(guild_id as i64)],
            )
            .map_err(|e| e.to_string())?
        };

        let rows: Vec<serde_json::Value> = match result {
            serde_json::Value::Array(arr) => arr,
            _ => vec![],
        };

        let welcome_settings = rows
            .first()
            .and_then(|r| r.get("settings"))
            .and_then(|s| s.get("welcome"))
            .and_then(|w| serde_json::from_value::<PluginWelcomeSettings>(w.clone()).ok())
            .unwrap_or_default();

        let enabled = welcome_settings.enabled.unwrap_or(false);
        let channel = welcome_settings
            .channel_id
            .unwrap_or_else(|| "Not set".into());
        let template = welcome_settings.template_id.unwrap_or_else(|| "1".into());
        let color = welcome_settings
            .primary_color
            .unwrap_or_else(|| "#5865F2".into());
        let msg_count = welcome_settings
            .messages
            .as_ref()
            .map(|m| m.len())
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

        Ok(ResponsePayload {
            content: Some(content),
            ephemeral: false,
            components_json: None,
            embed_json: None,
        })
    }
}
