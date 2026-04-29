//! Pure update logic for feed settings.
//!
//! Manages notification channel and role-permission toggles.

use crate::update::Update;

/// Messages that can mutate the feed-settings model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeedSettingsMsg {
    ToggleEnabled,
    SetChannel(Option<String>),
    SetSubRole(Option<String>),
    SetUnsubRole(Option<String>),
}

/// Commands returned by the update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedSettingsCmd {
    None,
}

/// The feed-settings model.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FeedSettingsModel {
    pub enabled: Option<bool>,
    pub channel_id: Option<String>,
    pub subscribe_role_id: Option<String>,
    pub unsubscribe_role_id: Option<String>,
}

impl FeedSettingsModel {
    pub fn is_enabled(&self) -> bool {
        self.enabled.unwrap_or(true)
    }
}

/// The update implementation for feed settings.
#[derive(Debug, Clone, Copy, Default)]
pub struct FeedSettingsUpdate;

impl FeedSettingsUpdate {
    pub fn new() -> Self {
        Self
    }
}

impl Update for FeedSettingsUpdate {
    type Model = FeedSettingsModel;
    type Msg = FeedSettingsMsg;
    type Cmd = FeedSettingsCmd;

    fn update(msg: Self::Msg, model: &mut Self::Model) -> Self::Cmd {
        use FeedSettingsMsg::*;

        match msg {
            ToggleEnabled => {
                let current = model.enabled.unwrap_or(true);
                model.enabled = Some(!current);
            }
            SetChannel(id) => {
                model.channel_id = id;
            }
            SetSubRole(id) => {
                model.subscribe_role_id = id;
            }
            SetUnsubRole(id) => {
                model.unsubscribe_role_id = id;
            }
        }
        FeedSettingsCmd::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── ToggleEnabled ───────────────────────────────────────────────────────

    #[test]
    fn test_toggle_enabled_from_true() {
        let mut model = FeedSettingsModel::default();
        assert!(model.is_enabled());

        let cmd = FeedSettingsUpdate::update(FeedSettingsMsg::ToggleEnabled, &mut model);

        assert_eq!(cmd, FeedSettingsCmd::None);
        assert!(!model.is_enabled());
    }

    #[test]
    fn test_toggle_enabled_from_false() {
        let mut model = FeedSettingsModel {
            enabled: Some(false),
            ..Default::default()
        };

        let cmd = FeedSettingsUpdate::update(FeedSettingsMsg::ToggleEnabled, &mut model);

        assert_eq!(cmd, FeedSettingsCmd::None);
        assert!(model.is_enabled());
    }

    // ── SetChannel ──────────────────────────────────────────────────────────

    #[test]
    fn test_set_channel() {
        let mut model = FeedSettingsModel::default();

        let cmd = FeedSettingsUpdate::update(
            FeedSettingsMsg::SetChannel(Some("123".to_string())),
            &mut model,
        );

        assert_eq!(cmd, FeedSettingsCmd::None);
        assert_eq!(model.channel_id, Some("123".to_string()));
    }

    #[test]
    fn test_set_channel_none() {
        let mut model = FeedSettingsModel {
            channel_id: Some("123".to_string()),
            ..Default::default()
        };

        let cmd = FeedSettingsUpdate::update(FeedSettingsMsg::SetChannel(None), &mut model);

        assert_eq!(cmd, FeedSettingsCmd::None);
        assert_eq!(model.channel_id, None);
    }

    // ── SetSubRole ──────────────────────────────────────────────────────────

    #[test]
    fn test_set_sub_role() {
        let mut model = FeedSettingsModel::default();

        let cmd = FeedSettingsUpdate::update(
            FeedSettingsMsg::SetSubRole(Some("role1".to_string())),
            &mut model,
        );

        assert_eq!(cmd, FeedSettingsCmd::None);
        assert_eq!(model.subscribe_role_id, Some("role1".to_string()));
    }

    // ── SetUnsubRole ────────────────────────────────────────────────────────

    #[test]
    fn test_set_unsub_role() {
        let mut model = FeedSettingsModel::default();

        let cmd = FeedSettingsUpdate::update(
            FeedSettingsMsg::SetUnsubRole(Some("role2".to_string())),
            &mut model,
        );

        assert_eq!(cmd, FeedSettingsCmd::None);
        assert_eq!(model.unsubscribe_role_id, Some("role2".to_string()));
    }

    // ── Model helpers ───────────────────────────────────────────────────────

    #[test]
    fn test_is_enabled_defaults_to_true() {
        let model = FeedSettingsModel::default();
        assert!(model.is_enabled());
    }

    #[test]
    fn test_model_default() {
        let model = FeedSettingsModel::default();
        assert_eq!(model.enabled, None);
        assert_eq!(model.channel_id, None);
        assert_eq!(model.subscribe_role_id, None);
        assert_eq!(model.unsubscribe_role_id, None);
    }
}
