//! Pure update logic for the `/plugins` admin surface.
//!
//! Manages which catalog plugins are enabled for a guild and which lifecycle
//! side effects (register, unregister, swap) the caller must perform.

use crate::update::Update;

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

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> PluginsModel {
        PluginsModel::new(vec!["hello".into(), "feed".into()], vec!["feed".into()])
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
}
