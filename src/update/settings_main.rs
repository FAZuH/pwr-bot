//! Pure update logic for the main settings page.
//!
//! Manages feature-enablement toggles.

use crate::update::Update;

/// Messages that can mutate the settings-main model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsMainMsg {
    ToggleFeeds,
    ToggleVoice,
    ToggleWelcome,
}

/// Commands returned by the update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsMainCmd {
    None,
}

/// The settings-main model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettingsMainModel {
    pub feeds_enabled: bool,
    pub voice_enabled: bool,
    pub welcome_enabled: bool,
    pub is_modified: bool,
}

impl SettingsMainModel {
    pub fn new(feeds: bool, voice: bool, welcome: bool) -> Self {
        Self {
            feeds_enabled: feeds,
            voice_enabled: voice,
            welcome_enabled: welcome,
            is_modified: false,
        }
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
        use SettingsMainMsg::*;

        match msg {
            ToggleFeeds => {
                model.feeds_enabled = !model.feeds_enabled;
                model.is_modified = true;
            }
            ToggleVoice => {
                model.voice_enabled = !model.voice_enabled;
                model.is_modified = true;
            }
            ToggleWelcome => {
                model.welcome_enabled = !model.welcome_enabled;
                model.is_modified = true;
            }
        }
        SettingsMainCmd::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_toggle_feeds() {
        let mut model = SettingsMainModel::new(false, false, false);
        assert!(!model.feeds_enabled);

        let cmd = SettingsMainUpdate::update(SettingsMainMsg::ToggleFeeds, &mut model);

        assert_eq!(cmd, SettingsMainCmd::None);
        assert!(model.feeds_enabled);
        assert!(model.is_modified);
    }

    #[test]
    fn test_toggle_voice() {
        let mut model = SettingsMainModel::new(false, true, false);

        let cmd = SettingsMainUpdate::update(SettingsMainMsg::ToggleVoice, &mut model);

        assert_eq!(cmd, SettingsMainCmd::None);
        assert!(!model.voice_enabled);
        assert!(model.is_modified);
    }

    #[test]
    fn test_toggle_welcome() {
        let mut model = SettingsMainModel::new(false, false, true);

        let cmd = SettingsMainUpdate::update(SettingsMainMsg::ToggleWelcome, &mut model);

        assert_eq!(cmd, SettingsMainCmd::None);
        assert!(!model.welcome_enabled);
        assert!(model.is_modified);
    }

    #[test]
    fn test_multiple_toggles() {
        let mut model = SettingsMainModel::new(true, true, true);

        SettingsMainUpdate::update(SettingsMainMsg::ToggleFeeds, &mut model);
        SettingsMainUpdate::update(SettingsMainMsg::ToggleVoice, &mut model);

        assert!(!model.feeds_enabled);
        assert!(!model.voice_enabled);
        assert!(model.welcome_enabled);
        assert!(model.is_modified);
    }

    #[test]
    fn test_is_modified_sticks() {
        let mut model = SettingsMainModel::new(false, false, false);
        assert!(!model.is_modified);

        SettingsMainUpdate::update(SettingsMainMsg::ToggleFeeds, &mut model);
        assert!(model.is_modified);

        // toggling back should keep is_modified true
        SettingsMainUpdate::update(SettingsMainMsg::ToggleFeeds, &mut model);
        assert!(model.is_modified);
    }

    #[test]
    fn test_new_preserves_initial_state() {
        let model = SettingsMainModel::new(true, false, true);
        assert!(model.feeds_enabled);
        assert!(!model.voice_enabled);
        assert!(model.welcome_enabled);
        assert!(!model.is_modified);
    }
}
