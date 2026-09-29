//! Pure update logic for the `/settings` command.
//!
//! Holds the single source of truth for the Settings view (`SettingsModel`),
//! the exhaustive message vocabulary (`SettingsMsg`), and an empty effect
//! vocabulary (`SettingsEffect`). The view is static — it lists the settings
//! sections the running core plugins declare and offers a back button — so
//! `update` never mutates the model and never performs IO. A section click
//! and Back are navigation exits, handled by the shell's `exit_navigation`
//! and the Router's session loop.
//!
//! IO (collecting the sections from the loaded plugin manifests) lives in the
//! shell layer ([`crate::bot::command::settings`]).

use crate::update::lifecycle::Lifecycle;

/// One Settings section: the tile the Settings view lists and a click
/// dispatches. The fields come verbatim from the plugin manifest's
/// declaration; `plugin` keys the manager lookup and `command` is the invoke
/// command of the panel the section opens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsSection {
    /// The plugin the section belongs to.
    pub plugin: String,
    /// The plugin's own command the section click invokes.
    pub command: String,
    /// The section's display name.
    pub name: String,
    /// The section's display description.
    pub description: String,
}

impl From<(String, pwr_plugin_protocol::manifest::SettingsSection)> for SettingsSection {
    fn from((plugin, section): (String, pwr_plugin_protocol::manifest::SettingsSection)) -> Self {
        Self {
            plugin,
            command: section.command,
            name: section.name,
            description: section.description,
        }
    }
}

impl SettingsSection {
    /// Builds a section from the manifest declaration's fields.
    pub fn new(
        plugin: impl Into<String>,
        command: impl Into<String>,
        name: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            plugin: plugin.into(),
            command: command.into(),
            name: name.into(),
            description: description.into(),
        }
    }
}

/// The Settings view model — the single source of truth for the view state.
#[derive(Debug, Clone)]
pub struct SettingsModel {
    sections: Vec<SettingsSection>,
}

impl SettingsModel {
    /// Constructs the model from the sections the shell collected.
    pub fn new(sections: Vec<SettingsSection>) -> Self {
        Self { sections }
    }

    /// The sections the view lists, in the order the shell supplied them.
    pub(crate) fn sections(&self) -> &[SettingsSection] {
        &self.sections
    }
}

/// Messages that drive the Settings view.
///
/// Exhaustive: every way the world can change the Settings model is one
/// variant. The lifecycle moments share the wrapped [`Lifecycle`] form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsMsg {
    /// The shared boot/timeout lifecycle moments.
    Lifecycle(Lifecycle),
    /// A section tile was pressed: the session exits to the section's panel.
    Section {
        /// The plugin the section belongs to.
        plugin: String,
        /// The plugin's command the section click invokes.
        command: String,
    },
    /// The back button was pressed: Root Back.
    Back,
}

impl From<Lifecycle> for SettingsMsg {
    fn from(lifecycle: Lifecycle) -> Self {
        Self::Lifecycle(lifecycle)
    }
}

/// Effects the Settings view can request.
///
/// Empty: the Settings view performs no side effects (sections are supplied
/// at model construction; navigation is a host concern handled via
/// [`SettingsMsg`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsEffect {}

/// The pure update function. The Settings view never mutates its model, so
/// every message is a no-op that returns no effects (expiry included — the
/// Settings view persists nothing).
pub fn update(msg: SettingsMsg, _model: &mut SettingsModel) -> Vec<SettingsEffect> {
    match msg {
        SettingsMsg::Lifecycle(lifecycle) => lifecycle.handle(Vec::new),
        SettingsMsg::Section { .. } | SettingsMsg::Back => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> SettingsModel {
        SettingsModel::new(vec![
            SettingsSection::new("feed", "feed-settings", "Feed", "Manage feeds"),
            SettingsSection::new("voice", "voice-settings", "Voice", "Manage voice tracking"),
        ])
    }

    #[test]
    fn start_is_a_noop() {
        let mut m = model();
        let effects = update(SettingsMsg::Lifecycle(Lifecycle::Start), &mut m);
        assert!(effects.is_empty());
    }

    #[test]
    fn expired_is_a_noop() {
        let mut m = model();
        let effects = update(SettingsMsg::Lifecycle(Lifecycle::Expired), &mut m);
        assert!(effects.is_empty());
    }

    #[test]
    fn a_section_click_is_a_noop() {
        let mut m = model();
        let effects = update(
            SettingsMsg::Section {
                plugin: "feed".into(),
                command: "feed-settings".into(),
            },
            &mut m,
        );
        assert!(effects.is_empty());
    }

    #[test]
    fn back_is_a_noop() {
        let mut m = model();
        let effects = update(SettingsMsg::Back, &mut m);
        assert!(effects.is_empty());
    }

    #[test]
    fn the_model_holds_the_sections_it_was_built_with() {
        let m = model();
        assert_eq!(m.sections().len(), 2);
        assert_eq!(m.sections()[0].plugin, "feed");
        assert_eq!(m.sections()[0].command, "feed-settings");
        assert_eq!(m.sections()[0].name, "Feed");
        assert_eq!(m.sections()[0].description, "Manage feeds");
    }
}
