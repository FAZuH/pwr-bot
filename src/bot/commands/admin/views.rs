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
use crate::bot::commands::Context;
use crate::bot::views::InteractiveView;
use crate::bot::views::ResponseKind;
use crate::bot::views::ResponseView;
use crate::bot::views::View;
use crate::database::model::ServerSettings;
use crate::database::model::ServerSettingsModel;
use crate::error::AppError;
use crate::view_core;

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
    ) -> Option<SettingsMainAction> {
        use SettingsMainAction::*;
        use SettingsMainState::*;

        match action {
            Feeds | Voice => match self.state {
                FeatureSettings => Some(action.clone()),
                DeactivateFeatures => {
                    self.toggle_features(from_ref(action));
                    Some(ToggleState)
                }
            },
            AddFeatures => {
                if let ComponentInteractionDataKind::StringSelect { values } = &interaction.data.kind {
                    let features: Vec<_> = values
                        .iter()
                        .filter_map(|val| self.core().registry.get(val).cloned())
                        .collect();
                    self.toggle_features(features);
                }
                Some(action.clone())
            }
            ToggleState => {
                self.state.toggle();
                Some(action.clone())
            }
            About => Some(action.clone()),
        }
    }
}
