//! Voice settings subcommand.

use std::time::Duration;

use serenity::all::ButtonStyle;
use serenity::all::CreateActionRow;
use serenity::all::CreateButton;
use serenity::all::CreateComponent;
use serenity::all::CreateContainer;
use serenity::all::CreateContainerComponent;
use serenity::all::CreateTextDisplay;

use crate::action_enum;
use crate::bot::commands::Context;
use crate::bot::commands::Error;
use crate::bot::commands::settings::SettingsPage;
use crate::bot::commands::settings::run_settings;
use crate::bot::controller::Controller;
use crate::bot::controller::Coordinator;
use crate::bot::error::BotError;
use crate::bot::navigation::NavigationResult;
use crate::bot::views::Action;
use crate::bot::views::ActionRegistry;
use crate::bot::views::ResponseKind;
use crate::bot::views::Trigger;
use crate::bot::views::ViewCommand;
use crate::bot::views::ViewContext;
use crate::bot::views::ViewEngine;
use crate::bot::views::ViewHandler;
use crate::bot::views::ViewRender;
use crate::controller;
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
    run_settings(ctx, Some(SettingsPage::Voice)).await
}

controller! { pub struct VoiceSettingsController<'a> {} }

#[async_trait::async_trait]
impl<'a, S: Send + Sync + 'static> Controller<S> for VoiceSettingsController<'a> {
    async fn run(
        &mut self,
        coordinator: &mut Coordinator<'_, S>,
    ) -> Result<NavigationResult, Error> {
        let ctx = *coordinator.context();
        ctx.defer().await?;
        let guild_id = ctx.guild_id().ok_or(BotError::GuildOnlyCommand)?.get();

        let service = ctx.data().service.voice_tracking.clone();

        let settings = service
            .get_server_settings(guild_id)
            .await
            .map_err(Error::from)?;

        let view = SettingsVoiceHandler { settings };

        let mut engine = ViewEngine::new(&ctx, view, Duration::from_secs(120));

        let nav = std::sync::Arc::new(std::sync::Mutex::new(NavigationResult::Exit));

        engine
            .run(|action| {
                let nav = nav.clone();
                Box::pin(async move {
                    match action {
                        SettingsVoiceAction::Back => {
                            *nav.lock().unwrap() = NavigationResult::Back;
                            ViewCommand::Exit
                        }
                        SettingsVoiceAction::About => {
                            *nav.lock().unwrap() = NavigationResult::SettingsAbout;
                            ViewCommand::Exit
                        }
                        SettingsVoiceAction::ToggleEnabled => ViewCommand::Render,
                    }
                })
            })
            .await?;

        // Save the settings once the run exits
        service
            .update_server_settings(guild_id, engine.handler.settings.clone())
            .await
            .map_err(Error::from)?;

        let res = nav.lock().unwrap().clone();
        Ok(res)
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
impl ViewHandler<SettingsVoiceAction> for SettingsVoiceHandler {
    async fn handle(
        &mut self,
        action: SettingsVoiceAction,
        _trigger: Trigger<'_>,
        _ctx: &ViewContext<'_, SettingsVoiceAction>,
    ) -> Result<ViewCommand, Error> {
        match action {
            SettingsVoiceAction::ToggleEnabled => {
                let current = self.settings.voice.enabled.unwrap_or(true);
                self.settings.voice.enabled = Some(!current);
                Ok(ViewCommand::Render)
            }
            SettingsVoiceAction::Back | SettingsVoiceAction::About => Ok(ViewCommand::Continue),
        }
    }
}

impl ViewRender<SettingsVoiceAction> for SettingsVoiceHandler {
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

        let enabled_button =
            CreateButton::new(registry.register(SettingsVoiceAction::ToggleEnabled))
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
                CreateButton::new(registry.register(SettingsVoiceAction::Back))
                    .label(SettingsVoiceAction::Back.label())
                    .style(ButtonStyle::Secondary),
                CreateButton::new(registry.register(SettingsVoiceAction::About))
                    .label(SettingsVoiceAction::About.label())
                    .style(ButtonStyle::Secondary),
            ]
            .into(),
        ));

        vec![container, nav_buttons].into()
    }
}
