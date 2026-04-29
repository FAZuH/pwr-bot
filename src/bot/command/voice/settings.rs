//! Voice settings subcommand.

use std::time::Duration;

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
    Coordinator::new(ctx).run(Navigation::SettingsVoice).await?;
    Ok(())
}

controller! { pub struct VoiceSettingsController<'a> {} }

#[async_trait::async_trait]
impl Controller for VoiceSettingsController<'_> {
    async fn run(&mut self, coordinator: std::sync::Arc<Coordinator<'_>>) -> Result<(), Error> {
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
    ) -> Result<ViewCommand, Error> {
        let ret = match ctx.action() {
            SettingsVoiceAction::ToggleEnabled => {
                let current = self.settings.voice.enabled.unwrap_or(true);
                self.settings.voice.enabled = Some(!current);
                ViewCommand::Render
            }
            SettingsVoiceAction::Back => {
                ctx.coordinator.navigate(Navigation::SettingsMain).await;
                ViewCommand::Exit
            }
            SettingsVoiceAction::About => {
                ctx.coordinator.navigate(Navigation::SettingsAbout).await;
                ViewCommand::Exit
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

        let enabled_button = registry
            .register(SettingsVoiceAction::ToggleEnabled)
            .as_button()
            .label(if is_enabled { "Disable" } else { "Enable" })
            .style(if is_enabled {
                ButtonStyle::Danger
            } else {
                ButtonStyle::Success
            });

        let container = CreateComponent::Container(CreateContainer::new(vec![
            CreateContainerComponent::TextDisplay(CreateTextDisplay::new(status_text)),
            CreateContainerComponent::ActionRow(CreateActionRow::Buttons(
                vec![enabled_button].into(),
            )),
        ]));

        let nav_buttons = CreateComponent::ActionRow(CreateActionRow::Buttons(
            vec![
                registry
                    .register(SettingsVoiceAction::Back)
                    .as_button()
                    .style(ButtonStyle::Secondary),
                registry
                    .register(SettingsVoiceAction::About)
                    .as_button()
                    .style(ButtonStyle::Secondary),
            ]
            .into(),
        ));

        vec![container, nav_buttons].into()
    }
}
