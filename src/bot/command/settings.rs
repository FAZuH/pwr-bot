//! Admin settings command.

use std::time::Duration;

use crate::bot::command::prelude::*;
use crate::entity::ServerSettings;
use crate::entity::ServerSettingsEntity;
use crate::update::Update;
use crate::update::settings_main::SettingsMainModel;
use crate::update::settings_main::SettingsMainMsg;
use crate::update::settings_main::SettingsMainUpdate;

/// Model representing a configurable feature in the bot.
///
/// Encapsulates feature identity, state access, and configuration logic
/// to decouple feature management from specific handler implementations.
pub struct Feature {
    /// Unique identifier for the feature (e.g., "feeds", "voice", "welcome")
    pub id: &'static str,
    /// Display label for the feature (e.g., "Feeds", "Voice", "Welcome")
    pub label: &'static str,
    /// Function to get the current enabled state from ServerSettings
    pub get_enabled: fn(&ServerSettings) -> bool,
    /// Function to set the enabled state in ServerSettings
    pub set_enabled: fn(&mut ServerSettings, bool),
    /// Navigation result when configuring this feature
    pub navigate: NavigationResult,
}

impl Feature {
    /// Get the current enabled state for this feature
    pub fn is_enabled(&self, settings: &ServerSettings) -> bool {
        (self.get_enabled)(settings)
    }

    /// Toggle the enabled state for this feature
    pub fn toggle_enabled(&self, settings: &mut ServerSettings) {
        let current = (self.get_enabled)(settings);
        (self.set_enabled)(settings, !current);
    }
}

/// Registry of all configurable features
pub struct FeatureRegistry;

impl FeatureRegistry {
    /// Returns all available features
    pub fn all() -> &'static [Feature] {
        static FEATURES: &[Feature] = &[
            Feature {
                id: "feeds",
                label: "Feeds",
                get_enabled: |s| s.feeds.enabled.unwrap_or(false),
                set_enabled: |s, v| s.feeds.enabled = Some(v),
                navigate: NavigationResult::SettingsFeeds,
            },
            Feature {
                id: "voice",
                label: "Voice",
                get_enabled: |s| s.voice.enabled.unwrap_or(false),
                set_enabled: |s, v| s.voice.enabled = Some(v),
                navigate: NavigationResult::SettingsVoice,
            },
            Feature {
                id: "welcome",
                label: "Welcome",
                get_enabled: |s| s.welcome.enabled.unwrap_or(false),
                set_enabled: |s, v| s.welcome.enabled = Some(v),
                navigate: NavigationResult::SettingsWelcome,
            },
        ];
        FEATURES
    }

    /// Find a feature by its ID
    pub fn find_by_id(id: &str) -> Option<&'static Feature> {
        Self::all().iter().find(|f| f.id == id)
    }

    /// Find a feature by its label
    pub fn find_by_label(label: &str) -> Option<&'static Feature> {
        Self::all().iter().find(|f| f.label == label)
    }
}

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

        let model = SettingsMainModel::new(
            settings.settings.0.feeds.enabled.unwrap_or(false),
            settings.settings.0.voice.enabled.unwrap_or(false),
            settings.settings.0.welcome.enabled.unwrap_or(false),
        );
        let view = SettingsMainHandler { settings, model };

        let mut engine = ViewEngine::new(ctx, view, Duration::from_secs(120), coordinator.clone());

        engine.run().await?;

        // Save settings if modified
        if engine.handler.model.is_modified {
            engine.handler.sync_model_to_settings();
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
    pub model: SettingsMainModel,
}

impl SettingsMainHandler {
    pub fn settings_mut(&mut self) -> &mut ServerSettings {
        &mut self.settings.settings.0
    }

    pub fn settings(&self) -> &ServerSettings {
        &self.settings.settings.0
    }

    pub fn done_update_settings(&mut self) -> Result<(), AppError> {
        if !self.model.is_modified {
            return Err(AppError::internal_with_ref(
                "done_update_settings called but settings not modified",
            ));
        }
        self.model.is_modified = false;

        Ok(())
    }

    fn sync_model_to_settings(&mut self) {
        self.settings.settings.0.feeds.enabled = Some(self.model.feeds_enabled);
        self.settings.settings.0.voice.enabled = Some(self.model.voice_enabled);
        self.settings.settings.0.welcome.enabled = Some(self.model.welcome_enabled);
    }
}

impl ViewRender for SettingsMainHandler {
    type Action = SettingsMainAction;
    fn render(&self, registry: &mut ActionRegistry<SettingsMainAction>) -> ResponseKind<'_> {
        let text_settings = CreateTextDisplay::new("-# **Settings**");
        let mut components = vec![CreateContainerComponent::TextDisplay(text_settings)];

        // Navigation section
        let text_configure = CreateTextDisplay::new(
            "### Configure Feature Settings
> 🛈  Click a button to edit settings for a specific feature.",
        );
        components.push(CreateContainerComponent::TextDisplay(text_configure));

        // Build navigation buttons for all features
        let navigation_buttons = CreateActionRow::Buttons(
            FeatureRegistry::all()
                .iter()
                .map(|feature| {
                    registry
                        .register(match feature.label {
                            "Feeds" => SettingsMainAction::FeedsFeature,
                            "Voice" => SettingsMainAction::VoiceFeature,
                            "Welcome" => SettingsMainAction::WelcomeFeature,
                            _ => SettingsMainAction::About, // Should never happen
                        })
                        .as_button()
                        .label(feature.label)
                        .style(ButtonStyle::Secondary)
                })
                .collect(),
        );
        components.push(CreateContainerComponent::ActionRow(navigation_buttons));

        // Toggle section
        let text_toggle = CreateTextDisplay::new(
            "### Enable or Disable Features
> 🛈  Turn features on or off. A checkmark means the feature is currently enabled.",
        );
        components.push(CreateContainerComponent::TextDisplay(text_toggle));

        // Build select menu options with emoji indicators using FeatureRegistry
        let select_options: Vec<_> = FeatureRegistry::all()
            .iter()
            .map(|feature| {
                let is_enabled = match feature.label {
                    "Feeds" => self.model.feeds_enabled,
                    "Voice" => self.model.voice_enabled,
                    "Welcome" => self.model.welcome_enabled,
                    _ => false,
                };
                let emoji = if is_enabled { "✅" } else { "⬜" };
                CreateSelectMenuOption::new(format!("{} {}", emoji, feature.label), feature.label)
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

#[async_trait::async_trait]
impl ViewHandler for SettingsMainHandler {
    type Action = SettingsMainAction;
    async fn handle(
        &mut self,
        ctx: ViewContext<'_, SettingsMainAction>,
    ) -> Result<ViewCommand, Error> {
        use SettingsMainAction::*;

        let cor = ctx.coordinator.clone();
        let action = ctx.action();
        match action {
            FeedsFeature => {
                if let Some(feature) = FeatureRegistry::find_by_label("Feeds") {
                    cor.navigate(feature.navigate.clone());
                }
                Ok(ViewCommand::Exit)
            }
            VoiceFeature => {
                if let Some(feature) = FeatureRegistry::find_by_label("Voice") {
                    cor.navigate(feature.navigate.clone());
                }
                Ok(ViewCommand::Exit)
            }
            WelcomeFeature => {
                if let Some(feature) = FeatureRegistry::find_by_label("Welcome") {
                    cor.navigate(feature.navigate.clone());
                }
                Ok(ViewCommand::Exit)
            }
            ToggleFeature => {
                if let Some(values) = ctx.string_select_values() {
                    for value in values {
                        match value.as_str() {
                            "Feeds" => {
                                SettingsMainUpdate::update(
                                    SettingsMainMsg::ToggleFeeds,
                                    &mut self.model,
                                );
                            }
                            "Voice" => {
                                SettingsMainUpdate::update(
                                    SettingsMainMsg::ToggleVoice,
                                    &mut self.model,
                                );
                            }
                            "Welcome" => {
                                SettingsMainUpdate::update(
                                    SettingsMainMsg::ToggleWelcome,
                                    &mut self.model,
                                );
                            }
                            _ => {}
                        }
                    }
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
