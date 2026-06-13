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

/// Pure update function for feed settings.
pub fn feed_settings_update(
    msg: FeedSettingsMsg,
    model: &mut FeedSettingsModel,
) -> FeedSettingsCmd {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggle_enabled_from_true() {
        let mut model = FeedSettingsModel::default();
        assert!(model.is_enabled());

        let cmd = feed_settings_update(FeedSettingsMsg::ToggleEnabled, &mut model);

        assert_eq!(cmd, FeedSettingsCmd::None);
        assert!(!model.is_enabled());
    }

    #[test]
    fn toggle_enabled_from_false() {
        let mut model = FeedSettingsModel {
            enabled: Some(false),
            ..Default::default()
        };

        let cmd = feed_settings_update(FeedSettingsMsg::ToggleEnabled, &mut model);

        assert_eq!(cmd, FeedSettingsCmd::None);
        assert!(model.is_enabled());
    }

    #[test]
    fn set_channel() {
        let mut model = FeedSettingsModel::default();

        let cmd = feed_settings_update(
            FeedSettingsMsg::SetChannel(Some("123".to_string())),
            &mut model,
        );

        assert_eq!(cmd, FeedSettingsCmd::None);
        assert_eq!(model.channel_id, Some("123".to_string()));
    }

    #[test]
    fn set_channel_none() {
        let mut model = FeedSettingsModel {
            channel_id: Some("123".to_string()),
            ..Default::default()
        };

        let cmd = feed_settings_update(FeedSettingsMsg::SetChannel(None), &mut model);

        assert_eq!(cmd, FeedSettingsCmd::None);
        assert_eq!(model.channel_id, None);
    }

    #[test]
    fn set_sub_role() {
        let mut model = FeedSettingsModel::default();

        let cmd = feed_settings_update(
            FeedSettingsMsg::SetSubRole(Some("role1".to_string())),
            &mut model,
        );

        assert_eq!(cmd, FeedSettingsCmd::None);
        assert_eq!(model.subscribe_role_id, Some("role1".to_string()));
    }

    #[test]
    fn set_unsub_role() {
        let mut model = FeedSettingsModel::default();

        let cmd = feed_settings_update(
            FeedSettingsMsg::SetUnsubRole(Some("role2".to_string())),
            &mut model,
        );

        assert_eq!(cmd, FeedSettingsCmd::None);
        assert_eq!(model.unsubscribe_role_id, Some("role2".to_string()));
    }

    #[test]
    fn is_enabled_defaults_to_true() {
        let model = FeedSettingsModel::default();
        assert!(model.is_enabled());
    }

    #[test]
    fn model_default() {
        let model = FeedSettingsModel::default();
        assert_eq!(model.enabled, None);
        assert_eq!(model.channel_id, None);
        assert_eq!(model.subscribe_role_id, None);
        assert_eq!(model.unsubscribe_role_id, None);
    }
}
