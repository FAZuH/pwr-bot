//! Pure update logic for the `/plugins` admin surface.
//!
//! Manages which catalog plugins are enabled for a guild and which lifecycle
//! side effects (register, unregister, swap) the caller must perform, and
//! holds the view state behind `/plugins list` (see [`PluginsListModel`]).

use crate::update::Update;
use crate::update::lifecycle::Lifecycle;

/// Messages that can mutate the plugins model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginsMsg {
    /// List catalog + enabled plugins; no mutation.
    List,
    /// Enable `name` for the guild (register).
    Enable(String),
    /// Disable `name` for the guild (unregister + unload).
    Disable(String),
    /// Swap `name` to a freshly installed binary.
    Swap(String),
}

/// Commands returned by the update, driving the caller's side effects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginsCmd {
    /// No side effect (no-op or pure list).
    None,
    /// Register the plugin's commands for the guild.
    Register(String),
    /// Unregister the plugin's commands for the guild and unload it.
    Unregister(String),
    /// Install a fresh binary and swap the running plugin onto it.
    Swap(String),
}

/// The plugins model: the catalog's known plugin names and the guild's
/// currently enabled subset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginsModel {
    /// Plugin names known to the catalog, in catalog order.
    pub catalog: Vec<String>,
    /// The guild's currently enabled subset of `catalog`.
    pub enabled: Vec<String>,
}

impl PluginsModel {
    /// Builds a model from the catalog names and the guild's enabled subset.
    pub fn new(catalog: Vec<String>, enabled: Vec<String>) -> Self {
        Self { catalog, enabled }
    }
}

/// The update implementation for the plugins admin surface.
#[derive(Debug, Clone, Copy, Default)]
pub struct PluginsUpdate;

impl Update for PluginsUpdate {
    type Model = PluginsModel;
    type Msg = PluginsMsg;
    type Cmd = PluginsCmd;

    fn update(msg: Self::Msg, model: &mut Self::Model) -> Self::Cmd {
        use PluginsCmd::*;

        match msg {
            PluginsMsg::List => None,
            PluginsMsg::Enable(name) => {
                if !model.catalog.contains(&name) || model.enabled.contains(&name) {
                    return None;
                }
                model.enabled.push(name.clone());
                Register(name)
            }
            PluginsMsg::Disable(name) => {
                if !model.enabled.contains(&name) {
                    return None;
                }
                model.enabled.retain(|enabled| enabled != &name);
                Unregister(name)
            }
            PluginsMsg::Swap(name) => {
                if model.enabled.contains(&name) {
                    Swap(name)
                } else {
                    None
                }
            }
        }
    }
}

/// The `/plugins list` view model: both plugin groups' rendered lines and
/// whether the internal group is on screen.
///
/// The lines arrive pre-rendered (the shell owns the per-plugin wording, which
/// the toggle must not change), so the core never needs the catalog or the
/// manifests and holds no serenity types. `show_internal` is session state: it
/// starts at whatever the command asked for — hidden unless the admin asked
/// otherwise — and never leaves the model, so the view forgets it on the next
/// `/plugins list`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginsListModel {
    internal: Vec<String>,
    catalog: Vec<String>,
    show_internal: bool,
}

impl PluginsListModel {
    /// Builds the model from the two groups' lines and the opening visibility the
    /// command asked for: `show_internal` seeds the button's state, which the
    /// toggle then flips from.
    pub fn new(internal: Vec<String>, catalog: Vec<String>, show_internal: bool) -> Self {
        Self {
            internal,
            catalog,
            show_internal,
        }
    }

    /// The internal plugins' lines, whether or not the group is on screen —
    /// the toggle only decides whether the view renders them.
    pub(crate) fn internal(&self) -> &[String] {
        &self.internal
    }

    /// The catalog plugins' lines. Always rendered: an empty group reads as
    /// "none configured", which an omitted group could not.
    pub(crate) fn catalog(&self) -> &[String] {
        &self.catalog
    }

    /// Whether the internal group is on screen.
    pub(crate) fn show_internal(&self) -> bool {
        self.show_internal
    }

    /// The Show/Hide Internal button's label for the state the view is in.
    pub(crate) fn toggle_label(&self) -> &'static str {
        if self.show_internal {
            "Hide Internal"
        } else {
            "Show Internal"
        }
    }
}

/// Messages that drive the `/plugins list` view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginsListMsg {
    /// The shared boot/timeout lifecycle moments.
    Lifecycle(Lifecycle),
    /// The Show/Hide Internal button was pressed.
    ToggleInternal,
}

impl From<Lifecycle> for PluginsListMsg {
    fn from(lifecycle: Lifecycle) -> Self {
        Self::Lifecycle(lifecycle)
    }
}

/// Effects the `/plugins list` view can request.
///
/// Empty: both groups arrive with the model and the toggle is a pure state
/// flip, so the view persists nothing and asks for nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginsListEffect {}

/// The pure update for the `/plugins list` view.
pub fn plugins_list_update(
    msg: PluginsListMsg,
    model: &mut PluginsListModel,
) -> Vec<PluginsListEffect> {
    match msg {
        PluginsListMsg::Lifecycle(lifecycle) => lifecycle.handle(Vec::new),
        PluginsListMsg::ToggleInternal => {
            model.show_internal = !model.show_internal;
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> PluginsModel {
        PluginsModel::new(vec!["hello".into(), "feed".into()], vec!["feed".into()])
    }

    fn list_model() -> PluginsListModel {
        PluginsListModel::new(
            vec!["`feed` — enabled, discord token".to_string()],
            vec!["`hello` — disabled, host ops only".to_string()],
            false,
        )
    }

    #[test]
    fn enable_adds_to_the_model_and_registers() {
        let mut model = model();
        let cmd = PluginsUpdate::update(PluginsMsg::Enable("hello".into()), &mut model);
        assert_eq!(cmd, PluginsCmd::Register("hello".into()));
        assert_eq!(model.enabled, vec!["feed".to_string(), "hello".to_string()]);
    }

    #[test]
    fn enable_of_an_unknown_plugin_is_a_noop() {
        let mut model = model();
        let cmd = PluginsUpdate::update(PluginsMsg::Enable("nope".into()), &mut model);
        assert_eq!(cmd, PluginsCmd::None);
        assert_eq!(model.enabled, vec!["feed".to_string()]);
    }

    #[test]
    fn enable_of_an_enabled_plugin_is_a_noop() {
        let mut model = model();
        let cmd = PluginsUpdate::update(PluginsMsg::Enable("feed".into()), &mut model);
        assert_eq!(cmd, PluginsCmd::None);
        assert_eq!(model.enabled, vec!["feed".to_string()]);
    }

    #[test]
    fn disable_removes_from_the_model_and_unregisters() {
        let mut model = model();
        let cmd = PluginsUpdate::update(PluginsMsg::Disable("feed".into()), &mut model);
        assert_eq!(cmd, PluginsCmd::Unregister("feed".into()));
        assert!(model.enabled.is_empty());
    }

    #[test]
    fn disable_of_a_disabled_plugin_is_a_noop() {
        let mut model = model();
        let cmd = PluginsUpdate::update(PluginsMsg::Disable("hello".into()), &mut model);
        assert_eq!(cmd, PluginsCmd::None);
        assert_eq!(model.enabled, vec!["feed".to_string()]);
    }

    #[test]
    fn swap_of_an_enabled_plugin_swaps() {
        let mut model = model();
        let cmd = PluginsUpdate::update(PluginsMsg::Swap("feed".into()), &mut model);
        assert_eq!(cmd, PluginsCmd::Swap("feed".into()));
        assert_eq!(model.enabled, vec!["feed".to_string()]);
    }

    #[test]
    fn swap_of_a_disabled_plugin_is_a_noop() {
        let mut model = model();
        let cmd = PluginsUpdate::update(PluginsMsg::Swap("hello".into()), &mut model);
        assert_eq!(cmd, PluginsCmd::None);
    }

    #[test]
    fn list_never_mutates() {
        let mut model = model();
        let cmd = PluginsUpdate::update(PluginsMsg::List, &mut model);
        assert_eq!(cmd, PluginsCmd::None);
        assert_eq!(model.catalog, vec!["hello".to_string(), "feed".to_string()]);
        assert_eq!(model.enabled, vec!["feed".to_string()]);
    }

    /// The internal group is hidden until the button is pressed, and pressing
    /// it again hides it: the flag is the whole session state, and it starts
    /// off so the list opens on the catalog alone.
    #[test]
    fn the_internal_group_starts_hidden_and_the_toggle_flips_it() {
        let mut model = list_model();

        assert!(!model.show_internal());
        assert_eq!(model.toggle_label(), "Show Internal");

        assert!(plugins_list_update(PluginsListMsg::ToggleInternal, &mut model).is_empty());
        assert!(model.show_internal());
        assert_eq!(model.toggle_label(), "Hide Internal");

        assert!(plugins_list_update(PluginsListMsg::ToggleInternal, &mut model).is_empty());
        assert!(!model.show_internal());
        assert_eq!(model.toggle_label(), "Show Internal");
    }

    /// The command argument seeds the opening state: `/plugin list` asked to
    /// show the internal plugins opens on them, and the toggle then hides them.
    /// Fails if the argument never reaches the model, or if it pins the toggle
    /// instead of seeding it.
    #[test]
    fn an_explicit_show_opens_on_the_internal_group_and_the_toggle_still_flips_it() {
        let mut model = PluginsListModel::new(
            vec!["`feed` — enabled, discord token".to_string()],
            vec!["`hello` — disabled, host ops only".to_string()],
            true,
        );

        assert!(model.show_internal());
        assert_eq!(model.toggle_label(), "Hide Internal");

        plugins_list_update(PluginsListMsg::ToggleInternal, &mut model);

        assert!(!model.show_internal());
        assert_eq!(model.toggle_label(), "Show Internal");
    }

    /// Toggling the group's visibility never touches its lines: the toggle
    /// hides what the view renders, so the plugin states an admin toggles back
    /// to are the same ones the list was built from.
    #[test]
    fn the_toggle_leaves_both_groups_lines_untouched() {
        let mut model = list_model();

        plugins_list_update(PluginsListMsg::ToggleInternal, &mut model);

        assert_eq!(model.internal(), ["`feed` — enabled, discord token"]);
        assert_eq!(model.catalog(), ["`hello` — disabled, host ops only"]);
    }

    /// An empty catalog is a group with no lines, not a missing group: the
    /// model keeps the catalog list it was built with, empty.
    #[test]
    fn an_empty_catalog_group_carries_no_lines_and_no_error() {
        let model = PluginsListModel::new(
            vec!["`feed` — enabled, discord token".to_string()],
            vec![],
            false,
        );

        assert!(model.catalog().is_empty());
        assert_eq!(model.internal(), ["`feed` — enabled, discord token"]);
    }

    /// The lifecycle moments are no-ops: the list view holds no side effects,
    /// so boot and timeout both leave it as it is.
    #[test]
    fn the_list_view_lifecycle_moments_are_noops() {
        let mut model = list_model();

        assert!(
            plugins_list_update(PluginsListMsg::Lifecycle(Lifecycle::Start), &mut model).is_empty()
        );
        assert!(!model.show_internal());

        plugins_list_update(PluginsListMsg::ToggleInternal, &mut model);
        assert!(
            plugins_list_update(PluginsListMsg::Lifecycle(Lifecycle::Expired), &mut model)
                .is_empty(),
            "expiry does not reset the session's toggle state"
        );
        assert!(model.show_internal());
    }
}
