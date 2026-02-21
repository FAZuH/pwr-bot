//! Voice settings subcommand.

use std::time::Duration;

use serenity::all::ButtonStyle;
use serenity::all::ComponentInteraction;
use serenity::all::CreateActionRow;
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
use crate::bot::views::InteractiveView;
use crate::bot::views::InteractiveViewBase;
use crate::bot::views::ResponseKind;
use crate::bot::views::ResponseView;
use crate::bot::views::View;
use crate::bot::views::ViewCommand;
use crate::bot::views::ViewCore;
use crate::bot::views::ViewHandler;
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

        let mut view = SettingsVoiceView::new(&ctx, settings);

        let nav = std::sync::Arc::new(std::sync::Mutex::new(NavigationResult::Exit));

        view.run(|action| {
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
            .update_server_settings(guild_id, view.handler.settings.clone())
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
        action: &SettingsVoiceAction,
        _interaction: &ComponentInteraction,
    ) -> Option<SettingsVoiceAction> {
        match action {
            SettingsVoiceAction::ToggleEnabled => {
                let current = self.settings.voice.enabled.unwrap_or(true);
                self.settings.voice.enabled = Some(!current);
                Some(action.clone())
            }
            SettingsVoiceAction::Back | SettingsVoiceAction::About => Some(action.clone()),
        }
    }
}

/// View for voice tracking settings.
pub struct SettingsVoiceView<'a> {
    pub base: InteractiveViewBase<'a, SettingsVoiceAction>,
    pub handler: SettingsVoiceHandler,
}

impl<'a> View<'a, SettingsVoiceAction> for SettingsVoiceView<'a> {
    fn core(&self) -> &ViewCore<'a, SettingsVoiceAction> {
        &self.base.core
    }
    fn core_mut(&mut self) -> &mut ViewCore<'a, SettingsVoiceAction> {
        &mut self.base.core
    }
    fn create_core(poise_ctx: &'a Context<'a>) -> ViewCore<'a, SettingsVoiceAction> {
        ViewCore::new(poise_ctx, Duration::from_secs(120))
    }
}

impl<'a> SettingsVoiceView<'a> {
    /// Creates a new voice settings view.
    pub fn new(ctx: &'a Context<'a>, settings: ServerSettings) -> Self {
        Self {
            base: InteractiveViewBase::new(Self::create_core(ctx)),
            handler: SettingsVoiceHandler { settings },
        }
    }
}

impl<'a> ResponseView<'a> for SettingsVoiceView<'a> {
    fn create_response<'b>(&mut self) -> ResponseKind<'b> {
        let is_enabled = self.handler.settings.voice.enabled.unwrap_or(true);

        let status_text = format!(
            "-# **Settings > Voice**\n## Voice Tracking Settings\n\n> 🛈  {}",
            if is_enabled {
                "Voice tracking is **active**."
            } else {
                "Voice tracking is **paused**."
            }
        );

        let enabled_button = self
            .base
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
                self.base
                    .register(SettingsVoiceAction::Back)
                    .as_button()
                    .style(ButtonStyle::Secondary),
                self.base
                    .register(SettingsVoiceAction::About)
                    .as_button()
                    .style(ButtonStyle::Secondary),
            ]
            .into(),
        ));

        vec![container, nav_buttons].into()
    }
}

crate::impl_interactive_view!(
    SettingsVoiceView<'a>,
    SettingsVoiceHandler,
    SettingsVoiceAction
);
