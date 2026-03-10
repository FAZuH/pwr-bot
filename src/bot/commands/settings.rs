//! Admin settings command.

use std::borrow::Cow;
use std::time::Duration;

use poise::serenity_prelude::*;

use crate::action_enum;
use crate::bot::checks::is_author_guild_admin;
use crate::bot::commands::Context;
use crate::bot::commands::Error;
use crate::bot::controller::Controller;
use crate::bot::coordinator::Coordinator;
use crate::bot::error::BotError;
use crate::bot::navigation::NavigationResult;
use crate::bot::views::Action;
use crate::bot::views::ActionRegistry;
use crate::bot::views::ResponseKind;
use crate::bot::views::ViewCommand;
use crate::bot::views::ViewContext;
use crate::bot::views::ViewEngine;
use crate::bot::views::ViewEvent;
use crate::bot::views::ViewHandler;
use crate::bot::views::ViewRender;
use crate::controller;
use crate::entity::ServerSettings;
use crate::entity::ServerSettingsEntity;
use crate::error::AppError;

/// Opens main server settings
///
/// Requires server administrator permissions.
#[poise::command(slash_command)]
pub async fn settings(ctx: Context<'_>) -> Result<(), Error> {
    Coordinator::new(ctx)
        .run(NavigationResult::SettingsMain)
        .await?;
    Ok(())
}

controller! { pub struct SettingsMainController<'a> {} }

#[async_trait::async_trait]
impl Controller for SettingsMainController<'_> {
    async fn run(&mut self, coordinator: std::sync::Arc<Coordinator<'_>>) -> Result<(), Error> {
        let ctx = *coordinator.context();
        is_author_guild_admin(ctx).await?;
        let guild_id = ctx.guild_id().ok_or(BotError::GuildOnlyCommand)?;

        let settings = ctx
            .data()
            .service
            .feed_subscription
            .get_server_settings(guild_id.into())
            .await?;

        let settings = ServerSettingsEntity {
            guild_id: guild_id.into(),
            settings: sqlx::types::Json(settings),
        };

        let view = SettingsMainHandler {
            settings,
            is_settings_modified: false,
        };

        let mut engine = ViewEngine::new(ctx, view, Duration::from_secs(120), coordinator.clone());

        engine.run().await?;

        // Save settings if modified
        if engine.handler.is_settings_modified {
            let guild_id = engine.handler.settings.guild_id;
            let settings_data = engine.handler.settings.settings.0.clone();
            ctx.data()
                .service
                .feed_subscription
                .update_server_settings(guild_id, settings_data)
                .await?;
            engine.handler.done_update_settings()?;
        }

        Ok(())
    }
}

pub struct SettingsMainHandler {
    pub settings: ServerSettingsEntity,
    pub is_settings_modified: bool,
}

impl SettingsMainHandler {
    pub fn settings_mut(&mut self) -> &mut ServerSettings {
        &mut self.settings.settings.0
    }

    pub fn settings(&self) -> &ServerSettings {
        &self.settings.settings.0
    }

    pub fn done_update_settings(&mut self) -> Result<(), AppError> {
        if !self.is_settings_modified {
            return Err(AppError::internal_with_ref(
                "done_update_settings called but settings not modified",
            ));
        }
        self.is_settings_modified = false;

        Ok(())
    }

    pub fn toggle_features<'b>(&mut self, features: impl Into<Cow<'b, [SettingsMainAction]>>) {
        let features = features.into();
        for feat in features.iter() {
            match feat {
                SettingsMainAction::FeedsFeature => {
                    self.settings_mut().feeds.enabled =
                        Some(!self.settings_mut().feeds.enabled.unwrap_or(false));
                    self.is_settings_modified = true;
                }
                SettingsMainAction::VoiceFeature => {
                    self.settings_mut().voice.enabled =
                        Some(!self.settings_mut().voice.enabled.unwrap_or(false));
                    self.is_settings_modified = true;
                }
                SettingsMainAction::WelcomeFeature => {
                    self.settings_mut().welcome.enabled =
                        Some(!self.settings_mut().welcome.enabled.unwrap_or(false));
                    self.is_settings_modified = true;
                }
                _ => {}
            }
        }
    }
}

impl ViewRender<SettingsMainAction> for SettingsMainHandler {
    fn render(&self, registry: &mut ActionRegistry<SettingsMainAction>) -> ResponseKind<'_> {
        let text_features = CreateTextDisplay::new(
            "-# **Settings**
### Features
> 🛈  Select a feature to toggle its enabled/disabled state. Checkmark indicates enabled features.",
        );

        let mut components = vec![CreateContainerComponent::TextDisplay(text_features)];

        // Get all features
        let all_features = vec![
            SettingsMainAction::FeedsFeature,
            SettingsMainAction::VoiceFeature,
            SettingsMainAction::WelcomeFeature,
        ];

        // Build select menu options with emoji indicators
        let select_options: Vec<_> = all_features
            .into_iter()
            .map(|feat| {
                let is_enabled = match &feat {
                    SettingsMainAction::FeedsFeature => self.settings().feeds.enabled.unwrap_or(false),
                    SettingsMainAction::VoiceFeature => self.settings().voice.enabled.unwrap_or(false),
                    SettingsMainAction::WelcomeFeature => self.settings().welcome.enabled.unwrap_or(false),
                    _ => false,
                };
                let emoji = if is_enabled { "✅" } else { "⬜" };
                CreateSelectMenuOption::new(
                    format!("{} {}", emoji, feat.label()),
                    feat.label(),
                )
            })
            .collect();

        // Add select menu - show disabled placeholder if empty (should never happen)
        let select_menu = if select_options.is_empty() {
            CreateActionRow::SelectMenu(
                CreateSelectMenu::new(
                    "placeholder_no_features",
                    CreateSelectMenuKind::String {
                        options: vec![CreateSelectMenuOption::new(
                            "No features available",
                            "placeholder",
                        )]
                        .into(),
                    },
                )
                .disabled(true),
            )
        } else {
            CreateActionRow::SelectMenu(
                registry
                    .register(SettingsMainAction::ToggleFeature)
                    .as_select(CreateSelectMenuKind::String {
                        options: select_options.into(),
                    }),
            )
        };
        components.push(CreateContainerComponent::ActionRow(select_menu));

        let container = CreateComponent::Container(CreateContainer::new(components));

        let bottom_buttons = CreateComponent::ActionRow(CreateActionRow::Buttons(
            vec![
                registry
                    .register(SettingsMainAction::About)
                    .as_button()
                    .style(ButtonStyle::Secondary),
            ]
            .into(),
        ));

        vec![container, bottom_buttons].into()
    }
}

action_enum! {
    SettingsMainAction {
        #[label = "Feeds"]
        FeedsFeature,
        #[label = "Voice"]
        VoiceFeature,
        #[label = "Welcome"]
        WelcomeFeature,
        ToggleFeature,
        #[label = "🛈 About"]
        About,
    }
}

impl SettingsMainAction {
    pub fn from_label(label: &str) -> Option<Self> {
        let ret = match label {
            "Feeds" => Self::FeedsFeature,
            "Voice" => Self::VoiceFeature,
            "Welcome" => Self::WelcomeFeature,
            _ => return None,
        };
        Some(ret)
    }
}

#[async_trait::async_trait]
impl ViewHandler<SettingsMainAction, ()> for SettingsMainHandler {
    async fn handle(
        &mut self,
        ctx: ViewContext<'_, SettingsMainAction>,
    ) -> Result<ViewCommand, Error> {
        use SettingsMainAction::*;

        let cor = ctx.coordinator.clone();
        let action = ctx.action();
        match action {
            FeedsFeature => {
                cor.navigate(NavigationResult::SettingsFeeds);
                Ok(ViewCommand::Exit)
            }
            VoiceFeature => {
                cor.navigate(NavigationResult::SettingsVoice);
                Ok(ViewCommand::Exit)
            }
            WelcomeFeature => {
                cor.navigate(NavigationResult::SettingsWelcome);
                Ok(ViewCommand::Exit)
            }
            ToggleFeature => {
                if let ViewEvent::Component(_, interaction) = ctx.event
                    && let ComponentInteractionDataKind::StringSelect { values } =
                        &interaction.data.kind
                {
                    let features: Vec<_> = values
                        .iter()
                        .filter_map(|v| SettingsMainAction::from_label(v))
                        .collect();
                    self.toggle_features(features);
                }
                Ok(ViewCommand::Render)
            }
            About => {
                cor.navigate(NavigationResult::SettingsAbout);
                Ok(ViewCommand::Exit)
            }
        }
    }
}
