//! Admin settings command.

use std::time::Duration;

use crate::bot::command::prelude::*;
use crate::bot::plugin::registry::PluginRegistry;
use crate::entity::Json;
use crate::entity::ServerSettings;
use crate::entity::ServerSettingsEntity;
use crate::update::Update;
use crate::update::settings_main::SettingsMainModel;
use crate::update::settings_main::SettingsMainMsg;
use crate::update::settings_main::SettingsMainUpdate;

/// Model representing a configurable feature in the bot.
#[derive(Clone)]
pub struct Feature {
    pub id: String,
    pub label: String,
    pub navigate: Navigation,
}

impl Feature {
    pub fn is_enabled(&self, settings: &ServerSettings) -> bool {
        settings.is_enabled(&self.id)
    }

    pub fn set_enabled(&self, settings: &mut ServerSettings, val: bool) {
        settings.set_enabled(&self.id, val);
    }
}

/// Collects features from the plugin registry's settings panels.
pub async fn collect_features(registry: &PluginRegistry) -> Vec<Feature> {
    let mut features = Vec::new();

    for (_plugin_name, _orig_name, panel) in registry.all_settings_panels().await {
        features.push(Feature {
            navigate: Navigation::SettingsPlugin {
                plugin_id: panel.id.clone(),
            },
            id: panel.id,
            label: panel.label,
        });
    }

    features
}

/// Builds the initial `SettingsMainModel` by reading enabled states from `ServerSettings`.
fn build_initial_model(settings: &ServerSettings, features: &[Feature]) -> SettingsMainModel {
    let mut map = std::collections::HashMap::new();
    for f in features {
        map.insert(f.id.clone(), f.is_enabled(settings));
    }
    SettingsMainModel::new(map)
}

/// Opens main server settings
///
/// Requires server administrator permissions.
#[poise::command(slash_command)]
pub async fn settings(ctx: Context<'_>) -> Result<(), Error> {
    Router::new(ctx).run(Navigation::SettingsMain).await?;
    Ok(())
}

handler! { pub struct SettingsMainHandler {} }

#[async_trait::async_trait]
impl CommandHandler for SettingsMainHandler {
    async fn run(&mut self, coordinator: std::sync::Arc<Router<'_>>) -> Result<(), Error> {
        let ctx = *coordinator.context();
        self.host_ctx.defer().await?;
        is_author_guild_admin(ctx).await?;
        let guild_id = ctx.guild_id().ok_or(BotError::GuildOnlyCommand)?;

        let settings = ctx
            .data()
            .service
            .settings
            .get_server_settings(guild_id.into())
            .await?;

        let features = collect_features(&ctx.data().plugin_registry).await;

        let settings = ServerSettingsEntity {
            guild_id: guild_id.get().into(),
            settings: Json(settings),
        };

        let model = build_initial_model(&settings.settings.0, &features);

        let view = SettingsMainView {
            settings,
            model,
            features,
        };

        let mut engine = ViewEngine::new(ctx, view, Duration::from_secs(120), coordinator.clone());

        engine.run().await?;

        // Save settings if modified
        if engine.handler.model.is_modified {
            engine.handler.sync_model_to_settings();
            let guild_id = engine.handler.settings.guild_id;
            let settings_data = engine.handler.settings.settings.0.clone();
            ctx.data()
                .service
                .settings
                .update_server_settings(*guild_id, settings_data)
                .await?;
            engine.handler.done_update_settings()?;
        }

        Ok(())
    }
}

pub struct SettingsMainView {
    pub settings: ServerSettingsEntity,
    pub model: SettingsMainModel,
    pub features: Vec<Feature>,
}

impl SettingsMainView {
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
        let Self {
            ref features,
            ref model,
            ref mut settings,
        } = *self;

        for f in features {
            let enabled = model.is_enabled(&f.id);
            settings.settings.0.set_enabled(&f.id, enabled);
        }
    }
}

impl ViewRender for SettingsMainView {
    type Action = SettingsMainAction;
    fn render(&self, registry: &mut ActionRegistry<SettingsMainAction>) -> ResponseKind<'_> {
        let mut components: Vec<CreateContainerComponent> =
            vec![CreateContainerComponent::TextDisplay(
                CreateTextDisplay::new("-# **Settings**"),
            )];

        // Navigation section
        let nav_label =
            "### Configure Feature Settings\n> 🛈  Select a feature to configure its settings.";
        components.push(CreateContainerComponent::TextDisplay(
            CreateTextDisplay::new(nav_label),
        ));

        // Navigation select menu
        if !self.features.is_empty() {
            let nav_options: Vec<CreateSelectMenuOption> = self
                .features
                .iter()
                .map(|f| CreateSelectMenuOption::new(f.label.as_str(), f.id.as_str()))
                .collect();

            let nav_select = registry
                .register(SettingsMainAction::NavigateToFeature)
                .as_select(CreateSelectMenuKind::String {
                    options: nav_options.into(),
                })
                .placeholder("Choose a feature to configure...");

            components.push(CreateContainerComponent::ActionRow(
                CreateActionRow::SelectMenu(nav_select),
            ));
        }

        // Toggle section
        let tog_label = "### Enable or Disable Features\n> 🛈  Turn features on or off. A checkmark means the feature is currently enabled.";
        components.push(CreateContainerComponent::TextDisplay(
            CreateTextDisplay::new(tog_label),
        ));

        let toggle_options: Vec<CreateSelectMenuOption> = self
            .features
            .iter()
            .map(|f| {
                let is_enabled = self.model.is_enabled(&f.id);
                let emoji = if is_enabled { "✅" } else { "⬜" };
                CreateSelectMenuOption::new(format!("{} {}", emoji, f.label), f.id.as_str())
            })
            .collect();

        if !toggle_options.is_empty() {
            let toggle_select = registry
                .register(SettingsMainAction::ToggleFeature)
                .as_select(CreateSelectMenuKind::String {
                    options: toggle_options.into(),
                });

            components.push(CreateContainerComponent::ActionRow(
                CreateActionRow::SelectMenu(toggle_select),
            ));
        }

        let container = CreateComponent::Container(CreateContainer::new(components));

        let about_button = registry
            .register(SettingsMainAction::About)
            .as_button()
            .style(ButtonStyle::Secondary);

        let bottom =
            CreateComponent::ActionRow(CreateActionRow::Buttons(vec![about_button].into()));

        vec![container, bottom].into()
    }
}

action_enum! {
    SettingsMainAction {
        /// Navigate to a selected feature's settings panel.
        NavigateToFeature,
        /// Toggle a selected feature on or off.
        ToggleFeature,
        #[label = "🛈 About"]
        About,
    }
}

#[async_trait::async_trait]
impl ViewHandler for SettingsMainView {
    type Action = SettingsMainAction;
    async fn handle(&mut self, ctx: ViewContext<'_, SettingsMainAction>) -> Result<ViewCmd, Error> {
        use SettingsMainAction::*;

        let cor = ctx.coordinator.clone();
        match ctx.action() {
            NavigateToFeature => {
                if let Some(values) = ctx.string_select_values()
                    && let Some(feature_id) = values.first()
                    && let Some(feature) = self.features.iter().find(|f| f.id == *feature_id)
                {
                    cor.navigate(feature.navigate.clone()).await;
                }
                Ok(ViewCmd::Exit)
            }
            ToggleFeature => {
                if let Some(values) = ctx.string_select_values() {
                    for id in values {
                        SettingsMainUpdate::update(SettingsMainMsg(id.clone()), &mut self.model);
                    }
                }
                Ok(ViewCmd::Render)
            }
            About => {
                cor.navigate(Navigation::SettingsAbout).await;
                Ok(ViewCmd::Exit)
            }
        }
    }
}
