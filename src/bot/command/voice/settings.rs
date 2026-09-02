//! Voice settings subcommand.

use std::time::Duration;

use pwr_ext::component;

use crate::bot::command::prelude::*;
use crate::entity::ServerSettings;

/// Configure voice tracking settings for this server
///
/// Enable or disable voice channel activity tracking.
/// Only server administrators can use this command.
#[poise::command(
    slash_command,
    default_member_permissions = "ADMINISTRATOR | MANAGE_GUILD"
)]
pub async fn settings(ctx: Context<'_>) -> Result<(), Error> {
    Router::new(ctx).run(Navigation::SettingsVoice).await?;
    Ok(())
}

handler! { pub struct VoiceSettingsHandler<'a> {} }

#[async_trait::async_trait]
impl CommandHandler for VoiceSettingsHandler<'_> {
    async fn run(&mut self, coordinator: std::sync::Arc<Router<'_>>) -> Result<(), Error> {
        let ctx = *coordinator.context();
        ctx.defer().await?;
        let guild_id = ctx.guild_id().ok_or(BotError::GuildOnlyCommand)?.get();

        let service = ctx.data().service.voice_tracking.clone();

        let settings = service
            .get_server_settings(guild_id)
            .await
            .map_err(Error::from)?;

        let view = SettingsVoiceHandler { settings };

        let mut engine = ViewEngine::new(ctx, view, Duration::from_secs(120), coordinator.clone());

        engine.run().await?;

        // Save the settings once the run exits
        service
            .update_server_settings(guild_id, engine.handler.settings.clone())
            .await
            .map_err(Error::from)?;

        Ok(())
    }
}

action_enum! {
    SettingsVoiceAction {
        ToggleEnabled,
        #[label = "❮ Back"]
        Back,
        #[label = "🛈 About"]
        About,
    }
}

pub struct SettingsVoiceHandler {
    pub settings: ServerSettings,
}

#[async_trait::async_trait]
impl ViewHandler for SettingsVoiceHandler {
    type Action = SettingsVoiceAction;
    async fn handle(
        &mut self,
        ctx: ViewContext<'_, SettingsVoiceAction>,
    ) -> Result<ViewCmd, Error> {
        let ret = match ctx.action() {
            SettingsVoiceAction::ToggleEnabled => {
                let current = self.settings.voice.enabled.unwrap_or(true);
                self.settings.voice.enabled = Some(!current);
                ViewCmd::Render
            }
            SettingsVoiceAction::Back => {
                ctx.coordinator.navigate(Navigation::SettingsMain).await;
                ViewCmd::Exit
            }
            SettingsVoiceAction::About => {
                ctx.coordinator.navigate(Navigation::SettingsAbout).await;
                ViewCmd::Exit
            }
        };
        Ok(ret)
    }
}

impl ViewRender for SettingsVoiceHandler {
    type Action = SettingsVoiceAction;
    fn render(&self, registry: &mut ActionRegistry<SettingsVoiceAction>) -> ResponseKind<'_> {
        let is_enabled = self.settings.voice.enabled.unwrap_or(true);

        let status_text = format!(
            "-# **Settings > Voice**\n## Voice Tracking Settings\n\n> 🛈  {}",
            if is_enabled {
                "Voice tracking is **active**."
            } else {
                "Voice tracking is **paused**."
            }
        );

        let enabled = registry.register(SettingsVoiceAction::ToggleEnabled);
        let enabled_label = if is_enabled { "Disable" } else { "Enable" };
        let enabled_style = if is_enabled {
            ButtonStyle::Danger
        } else {
            ButtonStyle::Success
        };

        let container = component! {
            container {
                text_display { content: status_text }
                action_row {
                    button {
                        custom_id: enabled.id,
                        label: enabled_label,
                        style: enabled_style
                    }
                }
            }
        };

        let back = registry.register(SettingsVoiceAction::Back);
        let about = registry.register(SettingsVoiceAction::About);

        let nav_buttons = component! {
            action_row {
                button {
                    custom_id: back.id,
                    label: back.label,
                    style: ButtonStyle::Secondary
                }
                button {
                    custom_id: about.id,
                    label: about.label,
                    style: ButtonStyle::Secondary
                }
            }
        };

        vec![
            CreateComponent::Container(container),
            CreateComponent::ActionRow(nav_buttons),
        ]
        .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bot::view::ActionRegistry;
    use crate::bot::view::ResponseKind;

    /// Rewrites every `custom_id` of the shape `Type:timestamp:counter` to a
    /// stable sentinel `id:Type`, so the rendered shape is reproducible across
    /// runs while still pinning kind/label/style/prefix/order.
    fn normalize_custom_ids(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(map) => {
                if let Some(serde_json::Value::String(cid)) = map.get("custom_id") {
                    let parts: Vec<&str> = cid.split(':').collect();
                    if parts.len() == 3
                        && parts[1].chars().all(|c| c.is_ascii_digit())
                        && parts[2].chars().all(|c| c.is_ascii_digit())
                    {
                        let replacement = serde_json::json!(format!("id:{}", parts[0]));
                        map.insert("custom_id".to_string(), replacement);
                    }
                }
                for v in map.values_mut() {
                    normalize_custom_ids(v);
                }
            }
            serde_json::Value::Array(arr) => {
                for v in arr {
                    normalize_custom_ids(v);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn voice_settings_render_snapshot() {
        let mut settings = ServerSettings::default();
        settings.voice.enabled = Some(true);
        let view = SettingsVoiceHandler { settings };
        let mut registry = ActionRegistry::new();
        let response = view.render(&mut registry);
        let ResponseKind::Component(components) = response else {
            panic!("expected a component response");
        };
        let mut value = serde_json::to_value(&components).unwrap();
        normalize_custom_ids(&mut value);
        assert_eq!(
            value,
            serde_json::json!([
                {
                    "type": 17,
                    "components": [
                        {
                            "type": 10,
                            "content": "-# **Settings > Voice**\n## Voice Tracking Settings\n\n> 🛈  Voice tracking is **active**."
                        },
                        {
                            "type": 1,
                            "components": [
                                {
                                    "type": 2,
                                    "custom_id": "id:SettingsVoiceAction",
                                    "disabled": false,
                                    "label": "Disable",
                                    "style": 4
                                }
                            ]
                        }
                    ]
                },
                {
                    "type": 1,
                    "components": [
                        {
                            "type": 2,
                            "custom_id": "id:SettingsVoiceAction",
                            "disabled": false,
                            "label": "❮ Back",
                            "style": 2
                        },
                        {
                            "type": 2,
                            "custom_id": "id:SettingsVoiceAction",
                            "disabled": false,
                            "label": "🛈 About",
                            "style": 2
                        }
                    ]
                }
            ])
        );
    }
}
