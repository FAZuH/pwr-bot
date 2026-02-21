//! Admin settings command.

use std::borrow::Cow;
use std::slice::from_ref;
use std::time::Duration;

use serenity::all::ButtonStyle;
use serenity::all::ComponentInteraction;
use serenity::all::ComponentInteractionDataKind;
use serenity::all::CreateActionRow;
use serenity::all::CreateButton;
use serenity::all::CreateComponent;
use serenity::all::CreateContainer;
use serenity::all::CreateContainerComponent;
use serenity::all::CreateSelectMenu;
use serenity::all::CreateSelectMenuKind;
use serenity::all::CreateSelectMenuOption;
use serenity::all::CreateTextDisplay;

use crate::action_enum;
use crate::bot::checks::is_author_guild_admin;
use crate::bot::commands::Context;
use crate::bot::commands::Error;
use crate::bot::commands::about::AboutController;
use crate::bot::commands::feed::settings::FeedSettingsController;
use crate::bot::commands::voice::settings::VoiceSettingsController;
use crate::bot::commands::welcome::WelcomeSettingsController;
use crate::bot::controller::Controller;
use crate::bot::controller::Coordinator;
use crate::bot::error::BotError;
use crate::bot::navigation::NavigationResult;
use crate::bot::views::InteractiveView;
use crate::bot::views::RenderExt;
use crate::bot::views::ResponseKind;
use crate::bot::views::ResponseView;
use crate::bot::views::View;
use crate::controller;
use crate::error::AppError;
use crate::model::ServerSettings;
use crate::model::ServerSettingsModel;
use crate::view_core;

/// Opens main server settings
///
/// Requires server administrator permissions.
#[poise::command(slash_command)]
pub async fn settings(ctx: Context<'_>) -> Result<(), Error> {
    run_settings(ctx, None).await
}

controller! { pub struct SettingsMainController<'a> {} }

#[async_trait::async_trait]
impl<'a, S: Send + Sync + 'static> Controller<S> for SettingsMainController<'a> {
    async fn run(
        &mut self,
        coordinator: &mut Coordinator<'_, S>,
    ) -> Result<NavigationResult, Error> {
        let ctx = *coordinator.context();
        is_author_guild_admin(ctx).await?;
        let guild_id = ctx.guild_id().ok_or(BotError::GuildOnlyCommand)?;

        let settings = ctx
            .data()
            .service
            .feed_subscription
            .get_server_settings(guild_id.into())
            .await?;

        let settings = ServerSettingsModel {
            guild_id: guild_id.into(),
            settings: sqlx::types::Json(settings),
        };

        let mut view = SettingsMainView::new(&ctx, settings);
        view.render().await?;

        while let Some((action, _)) = view.listen_once().await? {
            view.render().await?;
            if view.is_settings_modified {
                let guild_id = view.settings.guild_id;
                let settings = view.settings.settings.0.clone();
                ctx.data()
                    .service
                    .feed_subscription
                    .update_server_settings(guild_id, settings)
                    .await?;
                view.done_update_settings()?;
            }

            // Check for navigation actions
            match action {
                SettingsMainAction::Feeds => {
                    return Ok(NavigationResult::SettingsFeeds);
                }
                SettingsMainAction::Voice => {
                    return Ok(NavigationResult::SettingsVoice);
                }
                SettingsMainAction::Welcome => {
                    return Ok(NavigationResult::SettingsWelcome);
                }
                SettingsMainAction::About => {
                    return Ok(NavigationResult::SettingsAbout);
                }
                _ => {}
            }
        }

        Ok(NavigationResult::Exit)
    }
}
/// Tracks the current settings page for navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsPage {
    /// Main settings page
    Main,
    /// Feed settings page
    Feeds,
    /// Voice settings page
    Voice,
    /// Welcome settings page
    Welcome,
    /// About page
    About,
}

/// Runs the settings coordinator loop.
///
/// This is the entry point for the settings command. It creates a coordinator
/// and runs controllers in a loop based on their NavigationResult.
///
/// # Parameters
///
/// - `ctx`: The Discord command context
/// - `current_page`: Initial page to show. If None, defaults to [`SettingsPage::Main`]
pub async fn run_settings(
    ctx: Context<'_>,
    initial_page: Option<SettingsPage>,
) -> Result<(), Error> {
    let mut coordinator = Coordinator::new(ctx);
    let mut current_page = initial_page.unwrap_or(SettingsPage::Main);

    loop {
        // Create controller based on current page
        // Clone the context to avoid borrow checker issues
        let ctx_clone = *coordinator.context();
        let result = match current_page {
            SettingsPage::Main => {
                let mut controller = SettingsMainController::new(&ctx_clone);
                controller.run(&mut coordinator).await?
            }
            SettingsPage::Feeds => {
                let mut controller = FeedSettingsController::new(&ctx_clone);
                controller.run(&mut coordinator).await?
            }
            SettingsPage::Voice => {
                let mut controller = VoiceSettingsController::new(&ctx_clone);
                controller.run(&mut coordinator).await?
            }
            SettingsPage::Welcome => {
                let mut controller = WelcomeSettingsController::new(&ctx_clone);
                controller.run(&mut coordinator).await?
            }
            SettingsPage::About => {
                let mut controller = AboutController::new(&ctx_clone);
                controller.run(&mut coordinator).await?
            }
        };

        // Update current page based on navigation result
        match result {
            NavigationResult::SettingsMain => current_page = SettingsPage::Main,
            NavigationResult::SettingsFeeds => current_page = SettingsPage::Feeds,
            NavigationResult::SettingsVoice => current_page = SettingsPage::Voice,
            NavigationResult::SettingsWelcome => current_page = SettingsPage::Welcome,
            NavigationResult::SettingsAbout => current_page = SettingsPage::About,
            NavigationResult::Back => {
                // Go back to main from any sub-page
                current_page = SettingsPage::Main;
            }
            NavigationResult::Exit => break,
            // Other navigation variants not valid from settings
            _ => continue,
        }
    }

    Ok(())
}

pub enum SettingsMainState {
    FeatureSettings,
    DeactivateFeatures,
}

impl SettingsMainState {
    pub fn toggle(&mut self) {
        *self = match self {
            SettingsMainState::FeatureSettings => SettingsMainState::DeactivateFeatures,
            SettingsMainState::DeactivateFeatures => SettingsMainState::FeatureSettings,
        };
    }
}

view_core! {
    timeout = Duration::from_secs(120),
    /// Main settings view for managing server features.
    pub struct SettingsMainView<'a, SettingsMainAction> {
        pub state: SettingsMainState,
        pub settings: ServerSettingsModel,
        pub is_settings_modified: bool,
    }
}

impl<'a> SettingsMainView<'a> {
    pub fn new(ctx: &'a Context<'a>, settings: ServerSettingsModel) -> Self {
        Self {
            core: Self::create_core(ctx),
            state: SettingsMainState::FeatureSettings,
            settings,
            is_settings_modified: false,
        }
    }

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
                SettingsMainAction::Feeds => {
                    self.settings_mut().feeds.enabled =
                        Some(!self.settings_mut().feeds.enabled.unwrap_or(false));
                    self.is_settings_modified = true;
                }
                SettingsMainAction::Voice => {
                    self.settings_mut().voice.enabled =
                        Some(!self.settings_mut().voice.enabled.unwrap_or(false));
                    self.is_settings_modified = true;
                }
                SettingsMainAction::Welcome => {
                    self.settings_mut().welcome.enabled =
                        Some(!self.settings_mut().welcome.enabled.unwrap_or(false));
                    self.is_settings_modified = true;
                }
                _ => {}
            }
        }
    }

    fn get_active_features(&self) -> Vec<SettingsMainAction> {
        let mut features = Vec::new();
        if self.settings().voice.enabled.unwrap_or(false) {
            features.push(SettingsMainAction::Voice);
        }
        if self.settings().feeds.enabled.unwrap_or(false) {
            features.push(SettingsMainAction::Feeds);
        }
        if self.settings().welcome.enabled.unwrap_or(false) {
            features.push(SettingsMainAction::Welcome);
        }
        features
    }

    fn get_inactive_features(&self) -> Vec<SettingsMainAction> {
        let mut features = Vec::new();
        if !self.settings().voice.enabled.unwrap_or(false) {
            features.push(SettingsMainAction::Voice);
        }
        if !self.settings().feeds.enabled.unwrap_or(false) {
            features.push(SettingsMainAction::Feeds);
        }
        if !self.settings().welcome.enabled.unwrap_or(false) {
            features.push(SettingsMainAction::Welcome);
        }
        features
    }
}

impl<'a> ResponseView<'a> for SettingsMainView<'a> {
    fn create_response<'b>(&mut self) -> ResponseKind<'b> {
        let text_active_features_description = match &self.state {
            SettingsMainState::FeatureSettings => {
                "You can **configure** a feature by clicking the buttons below"
            }
            SettingsMainState::DeactivateFeatures => {
                "You can **disable** a feature by clicking the buttons below"
            }
        };
        let text_active_features = CreateTextDisplay::new(format!(
            "-# **Settings**
### Active Features
> 🛈  List of features currently active for this server.
> {text_active_features_description}."
        ));

        let active_features = self.get_active_features();
        let inactive_features = self.get_inactive_features();

        let mut components = vec![CreateContainerComponent::TextDisplay(text_active_features)];

        // Add button row - show disabled placeholder if empty
        let button_active_features = if active_features.is_empty() {
            CreateActionRow::Buttons(
                vec![
                    CreateButton::new("placeholder_no_features")
                        .label("No features enabled")
                        .style(ButtonStyle::Secondary)
                        .disabled(true),
                ]
                .into(),
            )
        } else {
            CreateActionRow::Buttons(
                active_features
                    .into_iter()
                    .map(|feat| {
                        self.register(feat).as_button().style(match &self.state {
                            SettingsMainState::FeatureSettings => ButtonStyle::Primary,
                            SettingsMainState::DeactivateFeatures => ButtonStyle::Danger,
                        })
                    })
                    .collect(),
            )
        };
        components.push(CreateContainerComponent::ActionRow(button_active_features));

        let button_toggle_state = CreateActionRow::Buttons(
            vec![
                self.register(SettingsMainAction::ToggleState)
                    .as_button()
                    .label(match &self.state {
                        SettingsMainState::FeatureSettings => "Deactivate Features",
                        SettingsMainState::DeactivateFeatures => "Feature Settings",
                    })
                    .style(match &self.state {
                        SettingsMainState::FeatureSettings => ButtonStyle::Danger,
                        SettingsMainState::DeactivateFeatures => ButtonStyle::Primary,
                    }),
            ]
            .into(),
        );
        components.push(CreateContainerComponent::ActionRow(button_toggle_state));

        let text_add_features = CreateTextDisplay::new(
            "### Add Features
> 🛈  List of inactive features that are available for this server.",
        );
        components.push(CreateContainerComponent::TextDisplay(text_add_features));

        // Add select menu - show disabled placeholder if empty
        let selectmenu_add_features = if inactive_features.is_empty() {
            CreateActionRow::SelectMenu(
                CreateSelectMenu::new(
                    "placeholder_no_inactive_features",
                    CreateSelectMenuKind::String {
                        options: vec![CreateSelectMenuOption::new(
                            "All features enabled",
                            "placeholder",
                        )]
                        .into(),
                    },
                )
                .disabled(true),
            )
        } else {
            CreateActionRow::SelectMenu(
                self.register(SettingsMainAction::AddFeatures).as_select(
                    CreateSelectMenuKind::String {
                        options: inactive_features
                            .into_iter()
                            .map(|feat| self.register(feat).as_select_option())
                            .collect(),
                    },
                ),
            )
        };
        components.push(CreateContainerComponent::ActionRow(selectmenu_add_features));

        let container = CreateComponent::Container(CreateContainer::new(components));

        let bottom_buttons = CreateComponent::ActionRow(CreateActionRow::Buttons(
            vec![
                self.register(SettingsMainAction::About)
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
        Feeds,
        Voice,
        Welcome,
        AddFeatures,
        ToggleState,
        #[label = "🛈 About"]
        About,
    }
}

#[async_trait::async_trait]
impl<'a> InteractiveView<'a, SettingsMainAction> for SettingsMainView<'a> {
    async fn handle(
        &mut self,
        action: &SettingsMainAction,
        interaction: &ComponentInteraction,
    ) -> Result<Option<SettingsMainAction>, Error> {
        use SettingsMainAction::*;
        use SettingsMainState::*;

        match action {
            Feeds | Voice | Welcome => match self.state {
                FeatureSettings => Ok(Some(action.clone())),
                DeactivateFeatures => {
                    self.toggle_features(from_ref(action));
                    Ok(Some(ToggleState))
                }
            },
            AddFeatures => {
                if let ComponentInteractionDataKind::StringSelect { values } =
                    &interaction.data.kind
                {
                    let features: Vec<_> = values
                        .iter()
                        .filter_map(|val| self.core().registry.get(val).cloned())
                        .collect();
                    self.toggle_features(features);
                }
                Ok(Some(action.clone()))
            }
            ToggleState => {
                self.state.toggle();
                Ok(Some(action.clone()))
            }
            About => Ok(Some(action.clone())),
        }
    }
}
