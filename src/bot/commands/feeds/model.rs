use serenity::all::ComponentInteraction;
use serenity::all::ComponentInteractionDataKind;

use crate::database::model::ServerSettings;

pub enum SettingsFeedsButton {
    Enabled(bool),
    Channel(Option<String>),
    SubRole(Option<String>),
    UnsubRole(Option<String>),
    None,
}

impl SettingsFeedsButton {
    pub const ENABLED_CID: &'static str = "feed_settings_enabled";
    pub const CHANNEL_CID: &'static str = "feed_settings_channel";
    pub const SUB_ROLE_CID: &'static str = "feed_settings_subrole";
    pub const UNSUB_ROLE_CID: &'static str = "feed_settings_unsubrole";
    pub const NONE_CID: &'static str = "feed_settings_none";
}

impl ServerSettings {
    pub fn update(&mut self, interaction: &ComponentInteraction) -> bool {
        let custom_id = &interaction.data.custom_id;
        match &interaction.data.kind {
            ComponentInteractionDataKind::StringSelect { values }
                if custom_id == "server_settings_enabled" =>
            {
                if let Some(value) = values.first() {
                    self.enabled = Some(value == "true");
                    return true;
                }
            }
            ComponentInteractionDataKind::ChannelSelect { values }
                if custom_id == "server_settings_channel" =>
            {
                self.channel_id = values.first().map(|id| id.to_string());
                return true;
            }
            ComponentInteractionDataKind::RoleSelect { values }
                if custom_id == "server_settings_sub_role" =>
            {
                self.subscribe_role_id = if values.is_empty() {
                    None
                } else {
                    values.first().map(|id| id.to_string())
                };
                return true;
            }
            ComponentInteractionDataKind::RoleSelect { values }
                if custom_id == "server_settings_unsub_role" =>
            {
                self.unsubscribe_role_id = if values.is_empty() {
                    None
                } else {
                    values.first().map(|id| id.to_string())
                };
                return true;
            }
            _ => {}
        }
        false
    }
}

use std::fmt::Display;

use poise::ChoiceParameter;

use crate::database::model::SubscriberType;
use crate::service::feed_subscription_service::SubscribeResult;
use crate::service::feed_subscription_service::UnsubscribeResult;
#[derive(ChoiceParameter)]
pub enum SendInto {
    Server,
    DM,
}

impl From<&SendInto> for SubscriberType {
    fn from(value: &SendInto) -> Self {
        match value {
            SendInto::DM => SubscriberType::Dm,
            SendInto::Server => SubscriberType::Guild,
        }
    }
}

impl Display for SendInto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DM => write!(f, "dm"),
            Self::Server => write!(f, "server"),
        }
    }
}

impl SendInto {
    pub fn name(&self) -> &'static str {
        match self {
            Self::DM => "DM",
            Self::Server => "Server",
        }
    }
}

impl Display for SubscribeResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let msg = match self {
            SubscribeResult::Success { feed } => format!(
                "✅ **Successfully** subscribed to [{}](<{}>)",
                feed.name, feed.source_url
            ),
            SubscribeResult::AlreadySubscribed { feed } => format!(
                "❌ You are **already subscribed** to [{}](<{}>)",
                feed.name, feed.source_url
            ),
        };
        write!(f, "{}", msg)
    }
}

impl Display for UnsubscribeResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let msg = match self {
            UnsubscribeResult::Success { feed } => format!(
                "✅ **Successfully** unsubscribed from [{}](<{}>)",
                feed.name, feed.source_url
            ),
            UnsubscribeResult::AlreadyUnsubscribed { feed } => format!(
                "❌ You are **not subscribed** to [{}](<{}>)",
                feed.name, feed.source_url
            ),
            UnsubscribeResult::NoneSubscribed { url } => {
                format!("❌ You are **not subscribed** to <{}>", url)
            }
        };
        write!(f, "{}", msg)
    }
}
