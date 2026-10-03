//! The `/plugins list` feature shell — a [`GuiFeature`] over the pure plugins
//! list core.
//!
//! Renders the two groups of plugins the host knows — the catalog group, and
//! the internal group behind a Show/Hide Internal button — with the button as
//! the last row. The feature holds no service: both groups arrive at model
//! construction (data-in via `Config`) with their per-plugin wording already
//! rendered by the command, and the button is a state flip inside the model.

use std::borrow::Cow;

use pwr_ext::component;

use crate::bot::command::prelude::*;
use crate::bot::gui::feature::GuiFeature;
use crate::bot::gui::feature::sealed;
use crate::bot::view::ActionRegistry;
use crate::bot::view::SelectValues;
use crate::update::PluginsListEffect;
use crate::update::PluginsListModel;
use crate::update::PluginsListMsg;
use crate::update::plugins_list_update;

/// The heading over the catalog plugins.
const CATALOG_HEADING: &str = "Catalog Plugins";

/// The heading over the internal plugins.
const INTERNAL_HEADING: &str = "Internal Plugins";

/// What a group with no plugins renders instead of its lines. Rendered for the
/// catalog group too: an absent catalog is a normal state, and a group that
/// disappeared would be indistinguishable from a command that never knew about
/// the catalog.
const NONE_CONFIGURED: &str = "none configured";

/// Data-in for the plugins list feature: the two groups' rendered lines.
pub struct PluginsListConfig {
    /// One line per internal plugin the host knows.
    pub internal: Vec<String>,
    /// One line per catalog plugin. Empty is a normal state.
    pub catalog: Vec<String>,
}

action_enum! {
    PluginsListAction {
        /// The Show/Hide Internal button: one action, toggled in the model.
        ///
        /// The rendered label follows the model (see
        /// [`PluginsListModel::toggle_label`]), so this variant carries no
        /// `#[label]`: one action has two labels, not two actions.
        ToggleInternal,
    }
}

/// The plugins list feature.
pub struct PluginsListFeature;

impl sealed::Sealed for PluginsListFeature {}

impl GuiFeature for PluginsListFeature {
    type Model = PluginsListModel;
    type Msg = PluginsListMsg;
    type Action = PluginsListAction;
    type Effect = PluginsListEffect;
    type Config = PluginsListConfig;

    fn initial(config: Self::Config) -> Self::Model {
        PluginsListModel::new(config.internal, config.catalog)
    }

    fn update(msg: Self::Msg, model: &mut Self::Model) -> Vec<Self::Effect> {
        plugins_list_update(msg, model)
    }

    fn view<'a>(
        model: &'a Self::Model,
        registry: &mut ActionRegistry<Self::Action>,
    ) -> Vec<CreateComponent<'a>> {
        let mut content = String::from("-# **Plugins**\n");
        content.push_str(&group(CATALOG_HEADING, model.catalog()));
        if model.show_internal() {
            content.push_str(&group(INTERNAL_HEADING, model.internal()));
        }

        let container: CreateComponent<'a> =
            CreateComponent::Container(CreateContainer::new(Cow::Owned(vec![
                CreateContainerComponent::TextDisplay(component! {
                    text_display { content: content }
                }),
            ])));

        // The toggle is the last row, so the button reads as the one control
        // the view offers rather than as one of the plugin rows.
        let toggle = registry.register(PluginsListAction::ToggleInternal);
        let toggle_row = CreateActionRow::Buttons(Cow::Owned(vec![
            CreateButton::new(toggle.id)
                .label(model.toggle_label().to_string())
                .style(ButtonStyle::Secondary),
        ]));

        vec![container, CreateComponent::ActionRow(toggle_row)]
    }

    fn translate(
        action: &Self::Action,
        _values: SelectValues,
        _model: &Self::Model,
    ) -> Option<Self::Msg> {
        match action {
            PluginsListAction::ToggleInternal => Some(PluginsListMsg::ToggleInternal),
        }
    }
}

/// One group of the view: a heading and its plugin lines, or the empty
/// sentence for a group with nothing in it.
fn group(heading: &str, lines: &[String]) -> String {
    let body: String = if lines.is_empty() {
        format!("{NONE_CONFIGURED}\n")
    } else {
        lines.iter().map(|line| format!("- {line}\n")).collect()
    };
    format!("## {heading}\n{body}")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::bot::gui::cycle;
    use crate::update::lifecycle::Lifecycle;

    fn config() -> PluginsListConfig {
        PluginsListConfig {
            internal: vec![
                "`feed` — enabled, discord token".to_string(),
                "`voice` — disabled, discord token".to_string(),
            ],
            catalog: vec![
                "`hello` — disabled, host ops only".to_string(),
                "`pro` — enabled, discord token".to_string(),
            ],
        }
    }

    /// The rendered text of the view, as the container's text display.
    fn content(model: &PluginsListModel) -> String {
        let value = cycle::capture::<PluginsListFeature>(model);
        value[0]["components"][0]["content"]
            .as_str()
            .expect("the container holds a text display")
            .to_string()
    }

    #[test]
    fn plugins_list_view_render_snapshot() {
        let mut model = PluginsListFeature::initial(config());
        plugins_list_update(PluginsListMsg::ToggleInternal, &mut model);

        let value = cycle::capture::<PluginsListFeature>(&model);

        let expected = json!([
            {
                "type": 17,
                "components": [
                    {
                        "type": 10,
                        "content": "-# **Plugins**\n## Catalog Plugins\n- `hello` — disabled, host ops only\n- `pro` — enabled, discord token\n## Internal Plugins\n- `feed` — enabled, discord token\n- `voice` — disabled, discord token\n"
                    }
                ]
            },
            {
                "type": 1,
                "components": [
                    {
                        "type": 2,
                        "custom_id": "id:PluginsListAction",
                        "disabled": false,
                        "label": "Hide Internal",
                        "style": 2
                    }
                ]
            }
        ]);

        assert_eq!(value, expected);
    }

    /// Both groups render with their per-plugin state, and the catalog group is
    /// rendered first. Fails if a group is dropped, or if the catalog is read
    /// from the internal sources.
    #[test]
    fn the_view_renders_both_groups_with_their_plugin_states() {
        let mut model = PluginsListFeature::initial(config());
        plugins_list_update(PluginsListMsg::ToggleInternal, &mut model);

        let content = content(&model);

        assert!(content.contains("## Catalog Plugins"), "{content}");
        assert!(content.contains("## Internal Plugins"), "{content}");
        assert!(
            content.contains("- `pro` — enabled, discord token"),
            "the catalog plugin keeps its state and authority: {content}"
        );
        assert!(
            content.contains("- `hello` — disabled, host ops only"),
            "{content}"
        );
        assert!(
            content.contains("- `feed` — enabled, discord token"),
            "the internal plugin keeps its state and authority: {content}"
        );
        assert!(
            content.find("## Catalog Plugins") < content.find("## Internal Plugins"),
            "the catalog group leads: {content}"
        );
    }

    /// An empty catalog still renders its group, with the empty sentence under
    /// it. Fails if the group is omitted, which an admin could not tell from a
    /// command that never learned about the catalog.
    #[test]
    fn an_empty_catalog_still_renders_its_group_with_none_configured() {
        let mut model = PluginsListFeature::initial(PluginsListConfig {
            internal: config().internal,
            catalog: vec![],
        });
        plugins_list_update(PluginsListMsg::ToggleInternal, &mut model);

        let content = content(&model);

        assert!(content.contains("## Catalog Plugins"), "{content}");
        assert!(
            content.contains("## Catalog Plugins\nnone configured\n"),
            "the empty catalog says so under its own heading: {content}"
        );
    }

    /// The button reads "Show Internal" while the internal group is hidden and
    /// "Hide Internal" once it is shown, so the label always states the action
    /// the press performs. Fails if the label is the action's static name, or
    /// if it is pinned to one state.
    #[test]
    fn the_button_label_states_the_press_it_performs() {
        let model = PluginsListFeature::initial(config());
        assert_eq!(model.toggle_label(), "Show Internal");
        let hidden = cycle::find_by_rendered_label::<PluginsListFeature>(&model, "Show Internal");

        let mut model = model;
        plugins_list_update(PluginsListMsg::ToggleInternal, &mut model);
        assert_eq!(model.toggle_label(), "Hide Internal");
        let shown = cycle::find_by_rendered_label::<PluginsListFeature>(&model, "Hide Internal");

        assert_eq!(
            (hidden, shown),
            (
                PluginsListAction::ToggleInternal,
                PluginsListAction::ToggleInternal
            ),
            "both labels are the one toggle action"
        );
    }

    /// The rendered button really sits at the bottom: the toggle is the last
    /// component, after the container holding both groups.
    #[test]
    fn the_toggle_is_the_last_rendered_component() {
        let model = PluginsListFeature::initial(config());

        let value = cycle::capture::<PluginsListFeature>(&model);

        assert_eq!(value.as_array().expect("a component list").len(), 2);
        let last = value[1].clone();
        assert_eq!(last["type"], 1, "the last component is the action row");
        assert_eq!(
            last["components"][0]["label"], "Show Internal",
            "the bottom row holds the toggle"
        );
    }

    /// The click goes through the whole cycle: the rendered button resolves
    /// through the registry to an action, the action translates to a message,
    /// and the pure update puts the internal group on screen. Fails if the
    /// button is a dead control, or if the toggle does not reach the view.
    #[test]
    fn clicking_the_button_toggles_the_internal_group_through_the_real_update() {
        let model = PluginsListFeature::initial(config());
        assert!(
            !content(&model).contains("## Internal Plugins"),
            "the internal group starts hidden: {}",
            content(&model)
        );

        let action = cycle::find_by_rendered_label::<PluginsListFeature>(&model, "Show Internal");
        let msg = cycle::translate_action::<PluginsListFeature>(&action, &model);
        assert_eq!(msg, PluginsListMsg::ToggleInternal);

        let mut model = model;
        assert!(
            PluginsListFeature::update(msg, &mut model).is_empty(),
            "the toggle asks for no side effects"
        );
        assert!(
            content(&model).contains("## Internal Plugins"),
            "the click put the group on screen: {}",
            content(&model)
        );

        // And back: the same button, now labelled the other way, hides it again.
        let action = cycle::find_by_rendered_label::<PluginsListFeature>(&model, "Hide Internal");
        let msg = cycle::translate_action::<PluginsListFeature>(&action, &model);
        PluginsListFeature::update(msg, &mut model);
        assert!(
            !content(&model).contains("## Internal Plugins"),
            "the second click hides it again: {}",
            content(&model)
        );
    }

    /// The toggle state is session state: a fresh `/plugins list` opens on the
    /// catalog alone however the last session left it, so nothing persists.
    #[test]
    fn a_fresh_list_starts_with_the_internal_group_hidden() {
        let mut model = PluginsListFeature::initial(config());
        plugins_list_update(PluginsListMsg::ToggleInternal, &mut model);
        assert!(content(&model).contains("## Internal Plugins"));

        let reopened = PluginsListFeature::initial(config());

        assert!(!content(&reopened).contains("## Internal Plugins"));
    }

    /// Boot and timeout speak the shared lifecycle vocabulary, and neither
    /// navigates: the toggle is the view's only control, so there is nowhere
    /// else to go.
    #[test]
    fn the_lifecycle_moments_are_noops_and_navigate_nowhere() {
        assert_eq!(
            PluginsListFeature::start_msg(),
            PluginsListMsg::Lifecycle(Lifecycle::Start)
        );
        assert_eq!(
            PluginsListFeature::timeout_msg(),
            PluginsListMsg::Lifecycle(Lifecycle::Expired)
        );
        assert_eq!(
            PluginsListFeature::exit_navigation(&PluginsListMsg::ToggleInternal),
            None
        );
        assert_eq!(
            PluginsListFeature::exit_navigation(&PluginsListMsg::Lifecycle(Lifecycle::Expired)),
            None
        );
    }
}
