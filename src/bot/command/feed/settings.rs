//! Feed settings subcommand.

use std::str::FromStr;
use std::time::Duration;

use pwr_ext::component;

use crate::bot::command::prelude::*;
use crate::entity::ServerSettings;
use crate::update::Update;
use crate::update::feed_settings::FeedSettingsModel;
use crate::update::feed_settings::FeedSettingsMsg;
use crate::update::feed_settings::FeedSettingsUpdate;

/// Configure feed settings for this server
///
/// Set up notification channels and required roles for feed subscriptions.
/// Only server administrators can use this command.
#[poise::command(
    slash_command,
    default_member_permissions = "ADMINISTRATOR | MANAGE_GUILD"
)]
pub async fn settings(ctx: Context<'_>) -> Result<(), Error> {
    Router::new(ctx).run(Navigation::SettingsFeeds).await?;
    Ok(())
}

handler! { pub struct FeedSettingsHandler<'a> {} }

#[async_trait::async_trait]
impl CommandHandler for FeedSettingsHandler<'_> {
    async fn run(&mut self, coordinator: std::sync::Arc<Router<'_>>) -> Result<(), Error> {
        let ctx = *coordinator.context();
        ctx.defer().await?;
        let service = ctx.data().service.feed_subscription.clone();

        let guild_id = ctx.guild_id().ok_or(BotError::GuildOnlyCommand)?.get();

        let mut settings = service.get_server_settings(guild_id).await?;

        let feeds_settings = settings.feeds.clone();
        let view = SettingsFeedHandler {
            model: FeedSettingsModel {
                enabled: feeds_settings.enabled,
                channel_id: feeds_settings.channel_id,
                subscribe_role_id: feeds_settings.subscribe_role_id,
                unsubscribe_role_id: feeds_settings.unsubscribe_role_id,
            },
            settings: &mut settings,
        };

        let mut engine = ViewEngine::new(ctx, view, Duration::from_secs(120), coordinator.clone());

        engine.run().await?;

        // Save settings after the view loop completes
        service
            .update_server_settings(guild_id, settings.clone())
            .await
            .ok();

        Ok(())
    }
}

action_enum! { SettingsFeedAction {
    Enabled,
    Channel,
    SubRole,
    UnsubRole,
    #[label = "❮ Back"]
    Back,
    #[label = "🛈 About"]
    About,
} }

pub struct SettingsFeedHandler<'a> {
    pub model: FeedSettingsModel,
    pub settings: &'a mut ServerSettings,
}

#[async_trait::async_trait]
impl<'a> ViewHandler for SettingsFeedHandler<'a> {
    type Action = SettingsFeedAction;
    async fn handle(&mut self, ctx: ViewContext<'_, SettingsFeedAction>) -> Result<ViewCmd, Error> {
        match ctx.action() {
            SettingsFeedAction::Enabled => {
                FeedSettingsUpdate::update(FeedSettingsMsg::ToggleEnabled, &mut self.model);
                self.settings.feeds.enabled = self.model.enabled;
                Ok(ViewCmd::Render)
            }
            SettingsFeedAction::Channel => {
                let channel_id = ctx
                    .channel_select_values()
                    .and_then(|v| v.first().map(|id| id.to_string()));
                FeedSettingsUpdate::update(
                    FeedSettingsMsg::SetChannel(channel_id),
                    &mut self.model,
                );
                self.settings.feeds.channel_id = self.model.channel_id.clone();
                Ok(ViewCmd::Render)
            }
            SettingsFeedAction::SubRole => {
                let role_id = ctx
                    .role_select_values()
                    .and_then(|v| v.first().map(|id| id.to_string()));
                FeedSettingsUpdate::update(FeedSettingsMsg::SetSubRole(role_id), &mut self.model);
                self.settings.feeds.subscribe_role_id = self.model.subscribe_role_id.clone();
                Ok(ViewCmd::Render)
            }
            SettingsFeedAction::UnsubRole => {
                let role_id = ctx
                    .role_select_values()
                    .and_then(|v| v.first().map(|id| id.to_string()));
                FeedSettingsUpdate::update(FeedSettingsMsg::SetUnsubRole(role_id), &mut self.model);
                self.settings.feeds.unsubscribe_role_id = self.model.unsubscribe_role_id.clone();
                Ok(ViewCmd::Render)
            }
            SettingsFeedAction::Back => {
                ctx.coordinator.navigate(Navigation::SettingsMain).await;
                Ok(ViewCmd::Exit)
            }
            SettingsFeedAction::About => {
                ctx.coordinator.navigate(Navigation::SettingsAbout).await;
                Ok(ViewCmd::Exit)
            }
        }
    }
}

impl<'a> SettingsFeedHandler<'a> {
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
}

impl<'a> ViewRender for SettingsFeedHandler<'a> {
    type Action = SettingsFeedAction;
    fn render(&self, registry: &mut ActionRegistry<SettingsFeedAction>) -> ResponseKind<'_> {
        let is_enabled = self.model.is_enabled();

        let status_text = format!(
            "-# **Settings > Feeds**\n## Feed Subscription Settings\n\n> 🛈  {}",
            if is_enabled {
                match &self.model.channel_id {
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
            default_channels: Some(Self::parse_channel_id(self.model.channel_id.as_ref()).into()),
        };
        let channel_placeholder = if self.model.channel_id.is_some() {
            "Change notification channel"
        } else {
            "⚠️ Required: Select a notification channel"
        };

        let sub_role_text = "### Subscribe Permission\n\n> 🛈  Who can add new feeds to this server. Leave empty to allow users with \"Manage Server\" permission.";
        let sub_role_action = registry.register(SettingsFeedAction::SubRole);
        let sub_role_kind = CreateSelectMenuKind::Role {
            default_roles: Some(Self::parse_role_id(self.model.subscribe_role_id.as_ref()).into()),
        };
        let sub_role_placeholder = if self.model.subscribe_role_id.is_some() {
            "Change subscribe role"
        } else {
            "Optional: Select role for subscribe permission"
        };

        let unsub_role_text = "### Unsubscribe Permission\n\n> 🛈  Who can remove feeds from this server. Leave empty to allow users with \"Manage Server\" permission.";
        let unsub_role_action = registry.register(SettingsFeedAction::UnsubRole);
        let unsub_role_kind = CreateSelectMenuKind::Role {
            default_roles: Some(
                Self::parse_role_id(self.model.unsubscribe_role_id.as_ref()).into(),
            ),
        };
        let unsub_role_placeholder = if self.model.unsubscribe_role_id.is_some() {
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
        .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bot::view::ActionRegistry;
    use crate::bot::view::ResponseKind;

    fn normalize_custom_ids(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(map) => {
                if let Some(serde_json::Value::String(cid)) = map.get("custom_id") {
                    let parts: Vec<&str> = cid.split(':').collect();
                    if parts.len() == 3
                        && parts[1].chars().all(|c| c.is_ascii_digit())
                        && parts[2].chars().all(|c| c.is_ascii_digit())
                    {
                        let replacement = serde_json::json!(format!("id:{}", parts[0]));
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
        let model = FeedSettingsModel {
            enabled: Some(true),
            channel_id: Some("123456789".to_string()),
            subscribe_role_id: Some("987654321".to_string()),
            unsubscribe_role_id: Some("987654322".to_string()),
        };
        let mut settings = ServerSettings::default();
        let view = SettingsFeedHandler {
            model,
            settings: &mut settings,
        };
        let mut registry = ActionRegistry::new();
        let response = view.render(&mut registry);
        let ResponseKind::Component(components) = response else {
            panic!("expected a component response");
        };
        let mut value = serde_json::to_value(&components).unwrap();
        normalize_custom_ids(&mut value);
        assert_eq!(
            value,
            serde_json::json!([
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
}
