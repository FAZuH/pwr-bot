//! The `/feed settings` feature shell — a [`GuiFeature`] over the pure feed
//! settings core.
//!
//! Renders the feed notification toggle, the notification-channel select, and
//! the subscribe/unsubscribe permission role selects. Drives the pure
//! transitions in `crate::update::feed_settings` and persists the settings
//! through a real [`EffectHandler`] adapter when the loop ends (back, about,
//! or timeout) — replacing the old handler's dual `FeedSettingsModel` +
//! `&mut ServerSettings` sources of truth and its implicit save-on-exit.

use std::str::FromStr;
use std::sync::Arc;

use pwr_ext::component;

use crate::bot::command::prelude::*;
use crate::bot::gui::effects::EffectHandler;
use crate::bot::gui::feature::GuiFeature;
use crate::bot::gui::feature::sealed;
use crate::bot::navigation::Navigation;
use crate::bot::view::SelectValues;
use crate::entity::ServerSettings;
use crate::service::traits::FeedSubscriptionProvider;
use crate::update::feed_settings::FeedSettingsEffect;
use crate::update::feed_settings::FeedSettingsModel;
use crate::update::feed_settings::FeedSettingsMsg;
use crate::update::feed_settings::update as feed_settings_update;

/// Data-in for the feed settings feature: the guild's loaded settings.
pub struct FeedSettingsConfig {
    pub settings: ServerSettings,
}

action_enum! {
    SettingsFeedAction {
        Enabled,
        Channel,
        SubRole,
        UnsubRole,
        #[label = "❮ Back"]
        Back,
        #[label = "🛈 About"]
        About,
    }
}

/// The feed settings feature.
pub struct FeedSettingsFeature;

impl sealed::Sealed for FeedSettingsFeature {}

impl GuiFeature for FeedSettingsFeature {
    type Model = FeedSettingsModel;
    type Msg = FeedSettingsMsg;
    type Action = SettingsFeedAction;
    type Effect = FeedSettingsEffect;
    type Config = FeedSettingsConfig;

    fn initial(config: Self::Config) -> Self::Model {
        FeedSettingsModel::new(config.settings)
    }

    fn start_msg() -> Self::Msg {
        FeedSettingsMsg::Start
    }

    fn timeout_msg() -> Self::Msg {
        FeedSettingsMsg::Expired
    }

    fn update(msg: Self::Msg, model: &mut Self::Model) -> Vec<Self::Effect> {
        feed_settings_update(msg, model)
    }

    fn view<'a>(
        model: &'a Self::Model,
        registry: &mut ActionRegistry<Self::Action>,
    ) -> Vec<CreateComponent<'a>> {
        let is_enabled = model.is_enabled();

        let status_text = format!(
            "-# **Settings > Feeds**\n## Feed Subscription Settings\n\n> 🛈  {}",
            if is_enabled {
                match model.channel_id() {
                    Some(id) => format!("Feed notifications are currently **active**. Notifications will be sent to <#{id}>"),
                    None => "Feed notifications are currently **active**, but notification channel is not set.".to_string(),
                }
            } else {
                "Feed notifications are currently **paused**. No notifications will be sent until it is re-enabled.".to_string()
            }
        );

        let enabled_action = registry.register(SettingsFeedAction::Enabled);
        let enabled_label = if is_enabled { "Disable" } else { "Enable" };
        let enabled_style = if is_enabled {
            ButtonStyle::Danger
        } else {
            ButtonStyle::Success
        };

        let channel_text =
            "### Notification Channel\n\n> 🛈  Choose where feed updates will be posted.";

        let channel_action = registry.register(SettingsFeedAction::Channel);
        let channel_kind = CreateSelectMenuKind::Channel {
            channel_types: Some(vec![ChannelType::Text, ChannelType::News].into()),
            default_channels: Some(parse_channel_id(model.channel_id().as_ref()).into()),
        };
        let channel_placeholder = if model.channel_id().is_some() {
            "Change notification channel"
        } else {
            "⚠️ Required: Select a notification channel"
        };

        let sub_role_text = "### Subscribe Permission\n\n> 🛈  Who can add new feeds to this server. Leave empty to allow users with \"Manage Server\" permission.";
        let sub_role_action = registry.register(SettingsFeedAction::SubRole);
        let sub_role_kind = CreateSelectMenuKind::Role {
            default_roles: Some(parse_role_id(model.subscribe_role_id().as_ref()).into()),
        };
        let sub_role_placeholder = if model.subscribe_role_id().is_some() {
            "Change subscribe role"
        } else {
            "Optional: Select role for subscribe permission"
        };

        let unsub_role_text = "### Unsubscribe Permission\n\n> 🛈  Who can remove feeds from this server. Leave empty to allow users with \"Manage Server\" permission.";
        let unsub_role_action = registry.register(SettingsFeedAction::UnsubRole);
        let unsub_role_kind = CreateSelectMenuKind::Role {
            default_roles: Some(parse_role_id(model.unsubscribe_role_id().as_ref()).into()),
        };
        let unsub_role_placeholder = if model.unsubscribe_role_id().is_some() {
            "Change unsubscribe role"
        } else {
            "Optional: Select role for unsubscribe permission"
        };

        let container = component! {
            container {
                text_display { content: status_text }
                action_row {
                    button {
                        custom_id: enabled_action.id,
                        label: enabled_label,
                        style: enabled_style
                    }
                }
                text_display { content: channel_text }
                action_row {
                    select_menu {
                        custom_id: channel_action.id,
                        kind: channel_kind,
                        placeholder: channel_placeholder
                    }
                }
                text_display { content: sub_role_text }
                action_row {
                    select_menu {
                        custom_id: sub_role_action.id,
                        kind: sub_role_kind,
                        min_values: 0,
                        placeholder: sub_role_placeholder
                    }
                }
                text_display { content: unsub_role_text }
                action_row {
                    select_menu {
                        custom_id: unsub_role_action.id,
                        kind: unsub_role_kind,
                        min_values: 0,
                        placeholder: unsub_role_placeholder
                    }
                }
            }
        };

        let back_action = registry.register(SettingsFeedAction::Back);
        let about_action = registry.register(SettingsFeedAction::About);

        let nav_buttons = component! {
            action_row {
                button {
                    custom_id: back_action.id,
                    label: back_action.label,
                    style: ButtonStyle::Secondary
                }
                button {
                    custom_id: about_action.id,
                    label: about_action.label,
                    style: ButtonStyle::Secondary
                }
            }
        };

        vec![
            CreateComponent::Container(container),
            CreateComponent::ActionRow(nav_buttons),
        ]
    }

    fn translate(
        action: &Self::Action,
        values: SelectValues,
        _model: &Self::Model,
    ) -> Option<Self::Msg> {
        match action {
            SettingsFeedAction::Enabled => Some(FeedSettingsMsg::ToggleEnabled),
            SettingsFeedAction::Channel => {
                let channel_id = match values {
                    SelectValues::Channel(v) => v.first().map(|id| id.to_string()),
                    _ => None,
                };
                Some(FeedSettingsMsg::SetChannel(channel_id))
            }
            SettingsFeedAction::SubRole => {
                let role_id = match values {
                    SelectValues::Role(v) => v.first().map(|id| id.to_string()),
                    _ => None,
                };
                Some(FeedSettingsMsg::SetSubRole(role_id))
            }
            SettingsFeedAction::UnsubRole => {
                let role_id = match values {
                    SelectValues::Role(v) => v.first().map(|id| id.to_string()),
                    _ => None,
                };
                Some(FeedSettingsMsg::SetUnsubRole(role_id))
            }
            SettingsFeedAction::Back => Some(FeedSettingsMsg::Back),
            SettingsFeedAction::About => Some(FeedSettingsMsg::About),
        }
    }

    fn exit_navigation(msg: &Self::Msg) -> Option<Navigation> {
        match msg {
            FeedSettingsMsg::Back => Some(Navigation::SettingsMain),
            FeedSettingsMsg::About => Some(Navigation::SettingsAbout),
            _ => None,
        }
    }
}

/// Parses a role ID string into a RoleId vector.
fn parse_role_id(id: Option<&String>) -> Vec<RoleId> {
    id.and_then(|id| RoleId::from_str(id).ok())
        .into_iter()
        .collect()
}

/// Parses a channel ID string into a GenericChannelId vector.
fn parse_channel_id(id: Option<&String>) -> Vec<GenericChannelId> {
    id.and_then(|id| ChannelId::from_str(id).ok().map(GenericChannelId::from))
        .into_iter()
        .collect()
}

/// The effect adapter that persists feed settings when the loop ends.
pub struct FeedSettingsEffectHandler {
    service: Arc<dyn FeedSubscriptionProvider>,
    guild_id: u64,
}

impl FeedSettingsEffectHandler {
    /// Creates an adapter bound to the guild's feed subscription service.
    pub fn new(service: Arc<dyn FeedSubscriptionProvider>, guild_id: u64) -> Self {
        Self { service, guild_id }
    }
}

impl EffectHandler for FeedSettingsEffectHandler {
    type Effect = FeedSettingsEffect;
    type Msg = FeedSettingsMsg;

    fn execute(
        &mut self,
        effect: FeedSettingsEffect,
        _tx: tokio::sync::mpsc::UnboundedSender<FeedSettingsMsg>,
    ) -> Vec<FeedSettingsMsg> {
        match effect {
            FeedSettingsEffect::PersistSettings(settings) => {
                let service = self.service.clone();
                let guild_id = self.guild_id;
                tokio::spawn(async move {
                    let _ = service.update_server_settings(guild_id, settings).await;
                });
                vec![]
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::bot::view::ActionRegistry;

    /// Rewrites every `custom_id` of the shape `Type:timestamp:counter` to a
    /// stable sentinel `id:Type`, so the rendered shape is reproducible across
    /// runs while still pinning kind/label/style/prefix/order.
    fn normalize_custom_ids(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(map) => {
                if let Some(serde_json::Value::String(cid)) = map.get("custom_id") {
                    let parts: Vec<&str> = cid.split(':').collect();
                    if parts.len() == 3
                        && parts[1].chars().all(|c| c.is_ascii_digit())
                        && parts[2].chars().all(|c| c.is_ascii_digit())
                    {
                        let replacement = json!(format!("id:{}", parts[0]));
                        map.insert("custom_id".to_string(), replacement);
                    }
                }
                for v in map.values_mut() {
                    normalize_custom_ids(v);
                }
            }
            serde_json::Value::Array(arr) => {
                for v in arr {
                    normalize_custom_ids(v);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn feed_settings_render_snapshot() {
        let mut settings = ServerSettings::default();
        settings.feeds.enabled = Some(true);
        settings.feeds.channel_id = Some("123456789".to_string());
        settings.feeds.subscribe_role_id = Some("987654321".to_string());
        settings.feeds.unsubscribe_role_id = Some("987654322".to_string());
        let model = FeedSettingsModel::new(settings);

        let mut registry = ActionRegistry::new();
        let components = FeedSettingsFeature::view(&model, &mut registry);
        let mut value = serde_json::to_value(&components).unwrap();
        normalize_custom_ids(&mut value);

        assert_eq!(
            value,
            json!([
                {
                    "type": 17,
                    "components": [
                        {
                            "type": 10,
                            "content": "-# **Settings > Feeds**\n## Feed Subscription Settings\n\n> 🛈  Feed notifications are currently **active**. Notifications will be sent to <#123456789>"
                        },
                        {
                            "type": 1,
                            "components": [
                                {
                                    "type": 2,
                                    "custom_id": "id:SettingsFeedAction",
                                    "disabled": false,
                                    "label": "Disable",
                                    "style": 4
                                }
                            ]
                        },
                        {
                            "type": 10,
                            "content": "### Notification Channel\n\n> 🛈  Choose where feed updates will be posted."
                        },
                        {
                            "type": 1,
                            "components": [
                                {
                                    "type": 8,
                                    "custom_id": "id:SettingsFeedAction",
                                    "channel_types": [0, 5],
                                    "placeholder": "Change notification channel",
                                    "default_values": [
                                        { "id": 123456789, "type": "channel" }
                                    ]
                                }
                            ]
                        },
                        {
                            "type": 10,
                            "content": "### Subscribe Permission\n\n> 🛈  Who can add new feeds to this server. Leave empty to allow users with \"Manage Server\" permission."
                        },
                        {
                            "type": 1,
                            "components": [
                                {
                                    "type": 6,
                                    "custom_id": "id:SettingsFeedAction",
                                    "min_values": 0,
                                    "placeholder": "Change subscribe role",
                                    "default_values": [
                                        { "id": 987654321, "type": "role" }
                                    ]
                                }
                            ]
                        },
                        {
                            "type": 10,
                            "content": "### Unsubscribe Permission\n\n> 🛈  Who can remove feeds from this server. Leave empty to allow users with \"Manage Server\" permission."
                        },
                        {
                            "type": 1,
                            "components": [
                                {
                                    "type": 6,
                                    "custom_id": "id:SettingsFeedAction",
                                    "min_values": 0,
                                    "placeholder": "Change unsubscribe role",
                                    "default_values": [
                                        { "id": 987654322, "type": "role" }
                                    ]
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
                            "custom_id": "id:SettingsFeedAction",
                            "disabled": false,
                            "label": "❮ Back",
                            "style": 2
                        },
                        {
                            "type": 2,
                            "custom_id": "id:SettingsFeedAction",
                            "disabled": false,
                            "label": "🛈 About",
                            "style": 2
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn feed_settings_disabled_snapshot() {
        let mut settings = ServerSettings::default();
        settings.feeds.enabled = Some(false);
        let model = FeedSettingsModel::new(settings);

        let mut registry = ActionRegistry::new();
        let components = FeedSettingsFeature::view(&model, &mut registry);
        let mut value = serde_json::to_value(&components).unwrap();
        normalize_custom_ids(&mut value);

        // The status text should reflect the paused state and the toggle
        // button should read "Enable" with a Success style.
        let containers = value[0]["components"].as_array().unwrap();
        let status = containers[0]["content"].as_str().unwrap();
        assert!(status.contains("Feed notifications are currently **paused**"));
        let toggle = containers[1]["components"][0].as_object().unwrap();
        assert_eq!(toggle["label"], "Enable");
        assert_eq!(toggle["style"], 3);
    }
}
