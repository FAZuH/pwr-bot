//! Pure update logic for welcome settings.
//!
//! Manages welcome-card configuration state and message-removal bookkeeping.

use std::collections::HashSet;

use crate::entity::WelcomeSettings;
use crate::update::Update;

/// Messages that can mutate the welcome-settings model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WelcomeSettingsMsg {
    ToggleEnabled,
    SetChannel(Option<String>),
    SetTemplate(Option<String>),
    MarkRemoval(HashSet<usize>),
    AddMessage(String),
    SetColor(String),
    SaveRemoval,
    CancelRemoval,
}

/// Commands returned by the update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WelcomeSettingsCmd {
    None,
    PersistSettings,
}

/// The welcome-settings model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WelcomeSettingsModel {
    pub settings: WelcomeSettings,
    pub marked_removal: HashSet<usize>,
}

impl WelcomeSettingsModel {
    pub fn new(settings: WelcomeSettings) -> Self {
        Self {
            settings,
            marked_removal: HashSet::new(),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.settings.enabled.unwrap_or(false)
    }

    pub fn message_count(&self) -> usize {
        self.settings
            .messages
            .as_ref()
            .map(|m| m.len())
            .unwrap_or(0)
    }
}

/// The update implementation for welcome settings.
#[derive(Debug, Clone, Copy, Default)]
pub struct WelcomeSettingsUpdate;

impl WelcomeSettingsUpdate {
    pub fn new() -> Self {
        Self
    }
}

impl Update for WelcomeSettingsUpdate {
    type Model = WelcomeSettingsModel;
    type Msg = WelcomeSettingsMsg;
    type Cmd = WelcomeSettingsCmd;

    fn update(msg: Self::Msg, model: &mut Self::Model) -> Self::Cmd {
        use WelcomeSettingsCmd::*;
        use WelcomeSettingsMsg::*;

        match msg {
            ToggleEnabled => {
                let current = model.settings.enabled.unwrap_or(false);
                model.settings.enabled = Some(!current);
                PersistSettings
            }
            SetChannel(channel_id) => {
                model.settings.channel_id = channel_id;
                PersistSettings
            }
            SetTemplate(template_id) => {
                model.settings.template_id = template_id;
                PersistSettings
            }
            MarkRemoval(indices) => {
                model.marked_removal = indices;
                None
            }
            AddMessage(msg) => {
                let trimmed = msg.trim().to_string();
                if !trimmed.is_empty() {
                    let msgs = model.settings.messages.get_or_insert_with(Vec::new);
                    if msgs.len() < 25 {
                        msgs.push(trimmed);
                    }
                }
                PersistSettings
            }
            SetColor(color) => {
                let trimmed = color.trim().to_string();
                if trimmed.starts_with('#') {
                    model.settings.primary_color = Some(trimmed);
                }
                PersistSettings
            }
            SaveRemoval => {
                let msgs = model.settings.messages.clone().unwrap_or_default();
                model.settings.messages = Some(
                    msgs.into_iter()
                        .enumerate()
                        .filter(|(i, _)| !model.marked_removal.contains(i))
                        .map(|(_, msg)| msg)
                        .collect(),
                );
                model.marked_removal.clear();
                PersistSettings
            }
            CancelRemoval => {
                model.marked_removal.clear();
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_model() -> WelcomeSettingsModel {
        WelcomeSettingsModel::new(WelcomeSettings::default())
    }

    // ── ToggleEnabled ───────────────────────────────────────────────────────

    #[test]
    fn test_toggle_enabled_from_false() {
        let mut model = empty_model();
        assert!(!model.is_enabled());

        let cmd = WelcomeSettingsUpdate::update(WelcomeSettingsMsg::ToggleEnabled, &mut model);

        assert_eq!(cmd, WelcomeSettingsCmd::PersistSettings);
        assert!(model.is_enabled());
    }

    #[test]
    fn test_toggle_enabled_from_true() {
        let mut model = empty_model();
        model.settings.enabled = Some(true);

        let cmd = WelcomeSettingsUpdate::update(WelcomeSettingsMsg::ToggleEnabled, &mut model);

        assert_eq!(cmd, WelcomeSettingsCmd::PersistSettings);
        assert!(!model.is_enabled());
    }

    // ── SetChannel ──────────────────────────────────────────────────────────

    #[test]
    fn test_set_channel() {
        let mut model = empty_model();

        let cmd = WelcomeSettingsUpdate::update(
            WelcomeSettingsMsg::SetChannel(Some("123".to_string())),
            &mut model,
        );

        assert_eq!(cmd, WelcomeSettingsCmd::PersistSettings);
        assert_eq!(model.settings.channel_id, Some("123".to_string()));
    }

    #[test]
    fn test_set_channel_none() {
        let mut model = empty_model();
        model.settings.channel_id = Some("123".to_string());

        let cmd = WelcomeSettingsUpdate::update(WelcomeSettingsMsg::SetChannel(None), &mut model);

        assert_eq!(cmd, WelcomeSettingsCmd::PersistSettings);
        assert_eq!(model.settings.channel_id, None);
    }

    // ── SetTemplate ─────────────────────────────────────────────────────────

    #[test]
    fn test_set_template() {
        let mut model = empty_model();

        let cmd = WelcomeSettingsUpdate::update(
            WelcomeSettingsMsg::SetTemplate(Some("5".to_string())),
            &mut model,
        );

        assert_eq!(cmd, WelcomeSettingsCmd::PersistSettings);
        assert_eq!(model.settings.template_id, Some("5".to_string()));
    }

    // ── MarkRemoval ─────────────────────────────────────────────────────────

    #[test]
    fn test_mark_removal() {
        let mut model = empty_model();
        let mut indices = HashSet::new();
        indices.insert(1);
        indices.insert(3);

        let cmd = WelcomeSettingsUpdate::update(
            WelcomeSettingsMsg::MarkRemoval(indices.clone()),
            &mut model,
        );

        assert_eq!(cmd, WelcomeSettingsCmd::None);
        assert_eq!(model.marked_removal, indices);
    }

    // ── AddMessage ──────────────────────────────────────────────────────────

    #[test]
    fn test_add_message() {
        let mut model = empty_model();

        let cmd = WelcomeSettingsUpdate::update(
            WelcomeSettingsMsg::AddMessage("Hello!".to_string()),
            &mut model,
        );

        assert_eq!(cmd, WelcomeSettingsCmd::PersistSettings);
        assert_eq!(model.message_count(), 1);
        assert_eq!(model.settings.messages.as_ref().unwrap()[0], "Hello!");
    }

    #[test]
    fn test_add_message_empty_ignored() {
        let mut model = empty_model();

        let cmd = WelcomeSettingsUpdate::update(
            WelcomeSettingsMsg::AddMessage("   ".to_string()),
            &mut model,
        );

        assert_eq!(cmd, WelcomeSettingsCmd::PersistSettings);
        assert_eq!(model.message_count(), 0);
    }

    #[test]
    fn test_add_message_cap_at_25() {
        let mut model = empty_model();
        model.settings.messages = Some((0..25).map(|i| format!("msg{}", i)).collect());

        let cmd = WelcomeSettingsUpdate::update(
            WelcomeSettingsMsg::AddMessage("overflow".to_string()),
            &mut model,
        );

        assert_eq!(cmd, WelcomeSettingsCmd::PersistSettings);
        assert_eq!(model.message_count(), 25);
    }

    // ── SetColor ────────────────────────────────────────────────────────────

    #[test]
    fn test_set_color_valid() {
        let mut model = empty_model();

        let cmd = WelcomeSettingsUpdate::update(
            WelcomeSettingsMsg::SetColor("#FF5733".to_string()),
            &mut model,
        );

        assert_eq!(cmd, WelcomeSettingsCmd::PersistSettings);
        assert_eq!(model.settings.primary_color, Some("#FF5733".to_string()));
    }

    #[test]
    fn test_set_color_invalid_ignored() {
        let mut model = empty_model();

        let cmd = WelcomeSettingsUpdate::update(
            WelcomeSettingsMsg::SetColor("FF5733".to_string()),
            &mut model,
        );

        assert_eq!(cmd, WelcomeSettingsCmd::PersistSettings);
        assert_eq!(model.settings.primary_color, None);
    }

    // ── SaveRemoval ─────────────────────────────────────────────────────────

    #[test]
    fn test_save_removal() {
        let mut model = empty_model();
        model.settings.messages = Some(vec![
            "a".to_string(),
            "b".to_string(),
            "c".to_string(),
            "d".to_string(),
        ]);
        model.marked_removal.insert(1);
        model.marked_removal.insert(3);

        let cmd = WelcomeSettingsUpdate::update(WelcomeSettingsMsg::SaveRemoval, &mut model);

        assert_eq!(cmd, WelcomeSettingsCmd::PersistSettings);
        assert!(model.marked_removal.is_empty());
        assert_eq!(
            model.settings.messages,
            Some(vec!["a".to_string(), "c".to_string()])
        );
    }

    #[test]
    fn test_save_removal_empty_messages() {
        let mut model = empty_model();
        model.marked_removal.insert(0);

        let cmd = WelcomeSettingsUpdate::update(WelcomeSettingsMsg::SaveRemoval, &mut model);

        assert_eq!(cmd, WelcomeSettingsCmd::PersistSettings);
        assert!(model.marked_removal.is_empty());
        assert_eq!(model.settings.messages, Some(vec![]));
    }

    // ── CancelRemoval ───────────────────────────────────────────────────────

    #[test]
    fn test_cancel_removal() {
        let mut model = empty_model();
        model.marked_removal.insert(1);
        model.marked_removal.insert(2);

        let cmd = WelcomeSettingsUpdate::update(WelcomeSettingsMsg::CancelRemoval, &mut model);

        assert_eq!(cmd, WelcomeSettingsCmd::None);
        assert!(model.marked_removal.is_empty());
    }

    // ── Model helpers ───────────────────────────────────────────────────────

    #[test]
    fn test_message_count() {
        let mut model = empty_model();
        assert_eq!(model.message_count(), 0);

        model.settings.messages = Some(vec!["a".to_string(), "b".to_string()]);
        assert_eq!(model.message_count(), 2);
    }

    #[test]
    fn test_is_enabled_default() {
        let model = empty_model();
        assert!(!model.is_enabled());
    }
}
