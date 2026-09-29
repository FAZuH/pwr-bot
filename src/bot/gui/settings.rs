//! The `/settings` feature shell — a [`GuiFeature`] over the pure settings
//! core.
//!
//! Renders the Settings list: one tile per section the running core plugins
//! declare in their manifests, plus the Root Back row. The feature holds no
//! service or manifest fetch: the sections arrive at model construction
//! (data-in via `Config`), and a section click is a navigation exit — the
//! Router resolves it into the Settings section handoff (see
//! [`crate::bot::command::session_exit`]).

use std::borrow::Cow;

use pwr_ext::component;

use crate::bot::command::prelude::*;
use crate::bot::gui::feature::GuiFeature;
use crate::bot::gui::feature::sealed;
use crate::bot::navigation::Navigation;
use crate::bot::view::ActionRegistry;
use crate::bot::view::SelectValues;
use crate::update::settings::SettingsEffect;
use crate::update::settings::SettingsModel;
use crate::update::settings::SettingsMsg;
use crate::update::settings::SettingsSection;
use crate::update::settings::update as settings_update;

/// Data-in for the settings feature: the sections collected from the loaded
/// plugin manifests before the host starts.
pub struct SettingsConfig {
    pub sections: Vec<SettingsSection>,
}

action_enum! {
    SettingsAction {
        /// A section tile: `plugin`/`command` identify the panel to open.
        #[label = "Open"]
        Section { plugin: String, command: String },
        #[label = "❮ Back"]
        Back,
    }
}

/// The settings feature.
pub struct SettingsFeature;

impl sealed::Sealed for SettingsFeature {}

impl GuiFeature for SettingsFeature {
    type Model = SettingsModel;
    type Msg = SettingsMsg;
    type Action = SettingsAction;
    type Effect = SettingsEffect;
    type Config = SettingsConfig;

    fn initial(config: Self::Config) -> Self::Model {
        SettingsModel::new(config.sections)
    }

    fn update(msg: Self::Msg, model: &mut Self::Model) -> Vec<Self::Effect> {
        settings_update(msg, model)
    }

    fn view<'a>(
        model: &'a Self::Model,
        registry: &mut ActionRegistry<Self::Action>,
    ) -> Vec<CreateComponent<'a>> {
        let mut section_lines = String::new();
        for section in model.sections() {
            section_lines.push_str(&format!(
                "- **{}** — {}\n",
                section.name, section.description
            ));
        }
        let content_text = if model.sections().is_empty() {
            "-# **Settings**\n## Server Settings\n\nNo settings sections are available.".to_string()
        } else {
            format!("-# **Settings**\n## Server Settings\n\n{section_lines}")
        };

        let section_row = (!model.sections().is_empty()).then(|| {
            let buttons: Vec<CreateButton<'a>> = model
                .sections()
                .iter()
                .map(|section| {
                    let action = registry.register(SettingsAction::Section {
                        plugin: section.plugin.clone(),
                        command: section.command.clone(),
                    });
                    CreateButton::new(action.id.clone())
                        .label(section.name.clone())
                        .style(ButtonStyle::Primary)
                })
                .collect();
            CreateContainerComponent::ActionRow(CreateActionRow::Buttons(Cow::Owned(buttons)))
        });

        let back_action = registry.register(SettingsAction::Back);

        let mut container_children: Vec<CreateContainerComponent<'a>> =
            vec![CreateContainerComponent::TextDisplay(component! {
                text_display { content: content_text }
            })];
        if let Some(row) = section_row {
            container_children.push(row);
        }
        let container: CreateComponent<'a> =
            CreateComponent::Container(CreateContainer::new(Cow::Owned(container_children)));

        let back_button = component! {
            action_row {
                button {
                    custom_id: back_action.id,
                    label: back_action.label,
                    style: ButtonStyle::Secondary
                }
            }
        };

        vec![container, CreateComponent::ActionRow(back_button)]
    }

    fn translate(
        action: &Self::Action,
        _values: SelectValues,
        _model: &Self::Model,
    ) -> Option<Self::Msg> {
        match action {
            SettingsAction::Section { plugin, command } => Some(SettingsMsg::Section {
                plugin: plugin.clone(),
                command: command.clone(),
            }),
            SettingsAction::Back => Some(SettingsMsg::Back),
        }
    }

    fn exit_navigation(msg: &Self::Msg) -> Option<Navigation> {
        match msg {
            SettingsMsg::Section { plugin, command } => Some(Navigation::SettingsSection {
                plugin: plugin.clone(),
                command: command.clone(),
            }),
            SettingsMsg::Back => Some(Navigation::Back),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::bot::gui::cycle;
    use crate::bot::view::normalize_custom_ids;
    use crate::update::lifecycle::Lifecycle;

    fn config() -> SettingsConfig {
        SettingsConfig {
            sections: vec![
                SettingsSection::new(
                    "feed",
                    "feed-settings",
                    "Feed",
                    "Manage feed subscription settings",
                ),
                SettingsSection::new(
                    "voice",
                    "voice-settings",
                    "Voice",
                    "Manage voice tracking settings",
                ),
            ],
        }
    }

    #[test]
    fn settings_view_render_snapshot() {
        let model = SettingsFeature::initial(config());

        let mut registry = ActionRegistry::new();
        let components = SettingsFeature::view(&model, &mut registry);
        let mut value = serde_json::to_value(&components).unwrap();
        normalize_custom_ids(&mut value);

        let expected = json!([
            {
                "type": 17,
                "components": [
                    {
                        "type": 10,
                        "content": "-# **Settings**\n## Server Settings\n\n- **Feed** — Manage feed subscription settings\n- **Voice** — Manage voice tracking settings\n"
                    },
                    {
                        "type": 1,
                        "components": [
                            {
                                "type": 2,
                                "custom_id": "id:SettingsAction",
                                "disabled": false,
                                "label": "Feed",
                                "style": 1
                            },
                            {
                                "type": 2,
                                "custom_id": "id:SettingsAction",
                                "disabled": false,
                                "label": "Voice",
                                "style": 1
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
                        "custom_id": "id:SettingsAction",
                        "disabled": false,
                        "label": "❮ Back",
                        "style": 2
                    }
                ]
            }
        ]);

        assert_eq!(value, expected);
    }

    #[test]
    fn an_empty_section_list_renders_the_empty_copy_without_a_tile_row() {
        let model = SettingsFeature::initial(SettingsConfig { sections: vec![] });

        let mut registry = ActionRegistry::new();
        let components = SettingsFeature::view(&model, &mut registry);
        let value = serde_json::to_value(&components).unwrap();

        let container = &value[0]["components"].as_array().unwrap();
        assert_eq!(container.len(), 1, "just the text display");
        assert!(
            container[0]["content"]
                .as_str()
                .unwrap()
                .contains("No settings sections are available")
        );
    }

    #[test]
    fn a_section_click_translates_and_exits_to_the_section_handoff() {
        let model = SettingsFeature::initial(config());

        let action = cycle::find_by_rendered_label::<SettingsFeature>(&model, "Feed");
        let msg = cycle::translate_action::<SettingsFeature>(&action, &model);
        assert_eq!(
            msg,
            SettingsMsg::Section {
                plugin: "feed".into(),
                command: "feed-settings".into()
            }
        );

        let mut model = model;
        let effects = SettingsFeature::update(msg, &mut model);
        assert!(effects.is_empty());
        assert_eq!(
            SettingsFeature::exit_navigation(&SettingsMsg::Section {
                plugin: "feed".into(),
                command: "feed-settings".into()
            }),
            Some(Navigation::SettingsSection {
                plugin: "feed".into(),
                command: "feed-settings".into()
            })
        );
    }

    #[test]
    fn back_translates_to_back_msg_and_exits_to_root_back() {
        let model = SettingsFeature::initial(config());

        let back = cycle::find_by_rendered_label::<SettingsFeature>(&model, "❮ Back");
        let msg = cycle::translate_action::<SettingsFeature>(&back, &model);
        assert_eq!(msg, SettingsMsg::Back);

        let mut model = model;
        let effects = SettingsFeature::update(msg, &mut model);
        assert!(effects.is_empty());
        assert_eq!(
            SettingsFeature::exit_navigation(&SettingsMsg::Back),
            Some(Navigation::Back)
        );
    }

    #[test]
    fn timeout_exits_without_navigating() {
        assert_eq!(
            SettingsFeature::timeout_msg(),
            SettingsMsg::Lifecycle(Lifecycle::Expired)
        );
        assert_eq!(
            SettingsFeature::exit_navigation(&SettingsMsg::Lifecycle(Lifecycle::Expired)),
            None
        );
    }

    #[test]
    fn a_section_click_leaves_the_rendered_view_unchanged() {
        let model = SettingsFeature::initial(config());
        let before = cycle::capture::<SettingsFeature>(&model);

        let action = cycle::find_by_rendered_label::<SettingsFeature>(&model, "Voice");
        let msg = cycle::translate_action::<SettingsFeature>(&action, &model);
        let mut model = model;
        SettingsFeature::update(msg, &mut model);

        assert_eq!(cycle::capture::<SettingsFeature>(&model), before);
    }
}
