//! Interactive feed settings view rendering.
//!
//! Builds Discord Components V2 messages for the feed settings panel, matching
//! the main branch layout with section headers, channel/role selectors, and
//! Save/Cancel buttons.

use std::str::FromStr;

use pwr_bot_sdk::*;
use serenity::all::*;

use crate::update::feed_settings::FeedSettingsModel;

/// Prefix for all custom IDs in the feed settings view.
pub const SETTINGS_CUSTOM_ID_PREFIX: &str = "feeds:";

/// State for the interactive feed settings view, keyed by guild_id.
#[derive(Clone)]
pub struct FeedSettingsState {
    pub model: FeedSettingsModel,
    pub guild_id: u64,
}

impl FeedSettingsState {
    pub fn new(model: FeedSettingsModel, guild_id: u64) -> Self {
        Self { model, guild_id }
    }
}

/// Renders the feed settings panel as a Components V2 [`ResponsePayload`].
pub fn render_feed_settings(state: &FeedSettingsState) -> ResponsePayload {
    let is_enabled = state.model.is_enabled();

    let status_text = format!(
        "-# **Settings > Feeds**\n## Feed Subscription Settings\n\n> 🛈  {}",
        if is_enabled {
            match &state.model.channel_id {
                Some(id) => {
                    format!(
                        "Feed notifications are currently **active**. Notifications will be sent to <#{id}>"
                    )
                }
                None => "Feed notifications are currently **active**, but notification channel is not set.".to_string(),
            }
        } else {
            "Feed notifications are currently **paused**. No notifications will be sent until it is re-enabled.".to_string()
        }
    );

    let enabled_label = if is_enabled { "Disable" } else { "Enable" };
    let toggle_button = CreateButton::new(format!("{SETTINGS_CUSTOM_ID_PREFIX}toggle"))
        .label(enabled_label)
        .style(if is_enabled {
            ButtonStyle::Danger
        } else {
            ButtonStyle::Success
        });

    let mut container_components: Vec<CreateContainerComponent> = vec![
        CreateContainerComponent::TextDisplay(CreateTextDisplay::new(status_text)),
        CreateContainerComponent::ActionRow(CreateActionRow::Buttons(vec![toggle_button].into())),
    ];

    // Channel select
    let default_channels: Vec<_> = state
        .model
        .channel_id
        .as_deref()
        .and_then(|id| GenericChannelId::from_str(id).ok())
        .map(|id| vec![id])
        .unwrap_or_default();

    container_components.push(CreateContainerComponent::TextDisplay(
        CreateTextDisplay::new(
            "### Notification Channel\n\n> 🛈  Choose where feed updates will be posted."
                .to_string(),
        ),
    ));

    let channel_select = CreateSelectMenu::new(
        format!("{SETTINGS_CUSTOM_ID_PREFIX}channel"),
        CreateSelectMenuKind::Channel {
            channel_types: Some(vec![ChannelType::Text, ChannelType::News].into()),
            default_channels: if default_channels.is_empty() {
                None
            } else {
                Some(default_channels.into())
            },
        },
    )
    .placeholder(if state.model.channel_id.is_some() {
        "Change notification channel"
    } else {
        "⚠️ Required: Select a notification channel"
    })
    .min_values(0)
    .max_values(1);

    container_components.push(CreateContainerComponent::ActionRow(
        CreateActionRow::SelectMenu(channel_select),
    ));

    // Subscribe role select
    container_components.push(CreateContainerComponent::TextDisplay(CreateTextDisplay::new(
        "### Subscribe Permission\n\n> 🛈  Who can add new feeds to this server. Leave empty to allow users with \"Manage Server\" permission.".to_string(),
    )));

    let default_sub_roles: Vec<_> = state
        .model
        .subscribe_role_id
        .as_deref()
        .and_then(|id| id.parse::<u64>().ok())
        .map(|id| vec![RoleId::new(id)])
        .unwrap_or_default();

    let sub_role_select = CreateSelectMenu::new(
        format!("{SETTINGS_CUSTOM_ID_PREFIX}sub_role"),
        CreateSelectMenuKind::Role {
            default_roles: if default_sub_roles.is_empty() {
                None
            } else {
                Some(default_sub_roles.into())
            },
        },
    )
    .placeholder(if state.model.subscribe_role_id.is_some() {
        "Change subscribe role"
    } else {
        "Optional: Select role for subscribe permission"
    })
    .min_values(0)
    .max_values(1);

    container_components.push(CreateContainerComponent::ActionRow(
        CreateActionRow::SelectMenu(sub_role_select),
    ));

    // Unsubscribe role select
    container_components.push(CreateContainerComponent::TextDisplay(CreateTextDisplay::new(
        "### Unsubscribe Permission\n\n> 🛈  Who can remove feeds from this server. Leave empty to allow users with \"Manage Server\" permission.".to_string(),
    )));

    let default_unsub_roles: Vec<_> = state
        .model
        .unsubscribe_role_id
        .as_deref()
        .and_then(|id| id.parse::<u64>().ok())
        .map(|id| vec![RoleId::new(id)])
        .unwrap_or_default();

    let unsub_role_select = CreateSelectMenu::new(
        format!("{SETTINGS_CUSTOM_ID_PREFIX}unsub_role"),
        CreateSelectMenuKind::Role {
            default_roles: if default_unsub_roles.is_empty() {
                None
            } else {
                Some(default_unsub_roles.into())
            },
        },
    )
    .placeholder(if state.model.unsubscribe_role_id.is_some() {
        "Change unsubscribe role"
    } else {
        "Optional: Select role for unsubscribe permission"
    })
    .min_values(0)
    .max_values(1);

    container_components.push(CreateContainerComponent::ActionRow(
        CreateActionRow::SelectMenu(unsub_role_select),
    ));

    // Save and Cancel buttons
    let save_btn = CreateButton::new(format!("{SETTINGS_CUSTOM_ID_PREFIX}save"))
        .label("Save")
        .style(ButtonStyle::Success);
    let cancel_btn = CreateButton::new(format!("{SETTINGS_CUSTOM_ID_PREFIX}cancel"))
        .label("Cancel")
        .style(ButtonStyle::Secondary);

    container_components.push(CreateContainerComponent::ActionRow(
        CreateActionRow::Buttons(vec![save_btn, cancel_btn].into()),
    ));

    let container = CreateContainer::new(container_components);
    let create_message = CreateMessage::new()
        .flags(MessageFlags::IS_COMPONENTS_V2)
        .components(vec![CreateComponent::Container(container)]);

    ResponsePayload::from_serializable(&create_message)
        .unwrap_or_else(|_| ResponsePayload::text("Error rendering feed settings"))
}
