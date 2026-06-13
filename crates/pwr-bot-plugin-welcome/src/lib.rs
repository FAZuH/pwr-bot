pub mod update;

use pwr_bot_sdk::*;

pub struct WelcomePlugin;

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

    fn settings_panels(&self) -> Vec<SettingsPanelSpec> {
        vec![SettingsPanelSpec::new("welcome", "Welcome")]
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

        Ok(ResponsePayload {
            content: Some(content),
            ephemeral: false,
            components_json: None,
            embed_json: None,
        })
    }
}
