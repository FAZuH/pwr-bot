//! Pure update logic for the main settings page.
//!
//! Manages feature-enablement toggles using a dynamic map of feature IDs.

use std::collections::HashMap;

use crate::update::Update;

/// Message to toggle a feature by its ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsMainMsg(pub String);

/// Commands returned by the update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsMainCmd {
    None,
}

/// The settings-main model backed by a feature ID → enabled map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsMainModel {
    pub features: HashMap<String, bool>,
    pub is_modified: bool,
}

impl SettingsMainModel {
    pub fn new(features: HashMap<String, bool>) -> Self {
        Self {
            features,
            is_modified: false,
        }
    }

    pub fn is_enabled(&self, id: &str) -> bool {
        self.features.get(id).copied().unwrap_or(false)
    }
}

/// The update implementation for the main settings page.
#[derive(Debug, Clone, Copy, Default)]
pub struct SettingsMainUpdate;

impl SettingsMainUpdate {
    pub fn new() -> Self {
        Self
    }
}

impl Update for SettingsMainUpdate {
    type Model = SettingsMainModel;
    type Msg = SettingsMainMsg;
    type Cmd = SettingsMainCmd;

    fn update(msg: Self::Msg, model: &mut Self::Model) -> Self::Cmd {
        let current = model.is_enabled(&msg.0);
        model.features.insert(msg.0, !current);
        model.is_modified = true;
        SettingsMainCmd::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model_with(a: bool, b: bool, c: bool) -> SettingsMainModel {
        let mut features = HashMap::new();
        features.insert("plugin_a".to_string(), a);
        features.insert("plugin_b".to_string(), b);
        features.insert("plugin_c".to_string(), c);
        SettingsMainModel::new(features)
    }

    #[test]
    fn toggle_plugin_a() {
        let mut model = model_with(false, false, false);
        assert!(!model.is_enabled("plugin_a"));

        let cmd = SettingsMainUpdate::update(SettingsMainMsg("plugin_a".into()), &mut model);

        assert_eq!(cmd, SettingsMainCmd::None);
        assert!(model.is_enabled("plugin_a"));
        assert!(model.is_modified);
    }

    #[test]
    fn toggle_plugin_b() {
        let mut model = model_with(false, true, false);

        let cmd = SettingsMainUpdate::update(SettingsMainMsg("plugin_b".into()), &mut model);

        assert_eq!(cmd, SettingsMainCmd::None);
        assert!(!model.is_enabled("plugin_b"));
        assert!(model.is_modified);
    }

    #[test]
    fn toggle_plugin_c() {
        let mut model = model_with(false, false, true);

        let cmd = SettingsMainUpdate::update(SettingsMainMsg("plugin_c".into()), &mut model);

        assert_eq!(cmd, SettingsMainCmd::None);
        assert!(!model.is_enabled("plugin_c"));
        assert!(model.is_modified);
    }

    #[test]
    fn multiple_toggles() {
        let mut model = model_with(true, true, true);

        SettingsMainUpdate::update(SettingsMainMsg("plugin_a".into()), &mut model);
        SettingsMainUpdate::update(SettingsMainMsg("plugin_b".into()), &mut model);

        assert!(!model.is_enabled("plugin_a"));
        assert!(!model.is_enabled("plugin_b"));
        assert!(model.is_enabled("plugin_c"));
        assert!(model.is_modified);
    }

    #[test]
    fn is_modified_sticks() {
        let mut model = model_with(false, false, false);
        assert!(!model.is_modified);

        SettingsMainUpdate::update(SettingsMainMsg("plugin_a".into()), &mut model);
        assert!(model.is_modified);

        SettingsMainUpdate::update(SettingsMainMsg("plugin_a".into()), &mut model);
        assert!(model.is_modified);
    }

    #[test]
    fn new_preserves_initial_state() {
        let model = model_with(true, false, true);
        assert!(model.is_enabled("plugin_a"));
        assert!(!model.is_enabled("plugin_b"));
        assert!(model.is_enabled("plugin_c"));
        assert!(!model.is_modified);
    }
}
