use std::str::FromStr;

use poise::CreateReply;
use serenity::all::ButtonStyle;
use serenity::all::ChannelId;
use serenity::all::ChannelType;
use serenity::all::CreateActionRow;
use serenity::all::CreateButton;
use serenity::all::CreateComponent;
use serenity::all::CreateContainer;
use serenity::all::CreateContainerComponent;
use serenity::all::CreateSection;
use serenity::all::CreateSectionAccessory;
use serenity::all::CreateSectionComponent;
use serenity::all::CreateSelectMenu;
use serenity::all::CreateSelectMenuKind;
use serenity::all::CreateSelectMenuOption;
use serenity::all::CreateTextDisplay;
use serenity::all::CreateThumbnail;
use serenity::all::CreateUnfurledMediaItem;
use serenity::all::GenericChannelId;
use serenity::all::MessageFlags;
use serenity::all::RoleId;

use crate::bot::commands::feeds::model::SettingsFeedsButton;
use crate::bot::views::PageNavigationView;
use crate::database::model::ServerSettings;
use crate::service::feed_subscription_service::Subscription;

pub struct SettingsFeedsView;

impl SettingsFeedsView {
    pub fn create_reply(settings: &ServerSettings) -> CreateReply<'_> {
        CreateReply::new()
            .flags(MessageFlags::IS_COMPONENTS_V2)
            .components(Self::create_components(settings))
    }

    pub fn create_components(settings: &ServerSettings) -> Vec<CreateComponent<'_>> {
        let is_enabled = settings.enabled.unwrap_or(true);

        let status_text = format!(
            "## Server Feed Settings\n\n> 🛈  {}",
            if is_enabled {
                format!(
                    "Feed notifications are currently active. Notifications will be sent to <#{}>",
                    settings.channel_id.as_deref().unwrap_or("Unknown")
                )
            } else {
                "Feed notifications are currently paused. No notifications will be sent until re-enabled.".to_string()
            }
        );

        let enabled_select = CreateSelectMenu::new(
            SettingsFeedsButton::ENABLED_CID,
            CreateSelectMenuKind::String {
                options: vec![
                    CreateSelectMenuOption::new("🟢 Enabled", "true").default_selection(is_enabled),
                    CreateSelectMenuOption::new("🔴 Disabled", "false")
                        .default_selection(!is_enabled),
                ]
                .into(),
            },
        )
        .placeholder("Toggle feed notifications");

        let channel_text =
            "### Notification Channel\n\n> 🛈  Choose where feed updates will be posted.";

        let channel_select = CreateSelectMenu::new(
            SettingsFeedsButton::CHANNEL_CID,
            CreateSelectMenuKind::Channel {
                channel_types: Some(vec![ChannelType::Text, ChannelType::News].into()),
                default_channels: Some(Self::parse_channel_id(settings.channel_id.as_ref()).into()),
            },
        )
        .placeholder(if settings.channel_id.is_some() {
            "Change notification channel"
        } else {
            "⚠️ Required: Select a notification channel"
        });

        let sub_role_text = "### Subscribe Permission\n\n> 🛈  Who can add new feeds to this server. Leave empty to allow users with \"Manage Server\" permission.";
        let sub_role_select = CreateSelectMenu::new(
            SettingsFeedsButton::SUB_ROLE_CID,
            CreateSelectMenuKind::Role {
                default_roles: Some(
                    Self::parse_role_id(settings.subscribe_role_id.as_ref()).into(),
                ),
            },
        )
        .min_values(0)
        .placeholder(if settings.subscribe_role_id.is_some() {
            "Change subscribe role"
        } else {
            "Optional: Select role for subscribe permission"
        });

        let unsub_role_text = "### Unsubscribe Permission\n\n> 🛈  Who can remove feeds from this server. Leave empty to allow users with \"Manage Server\" permission.";
        let unsub_role_select = CreateSelectMenu::new(
            SettingsFeedsButton::UNSUB_ROLE_CID,
            CreateSelectMenuKind::Role {
                default_roles: Some(
                    Self::parse_role_id(settings.unsubscribe_role_id.as_ref()).into(),
                ),
            },
        )
        .min_values(0)
        .placeholder(if settings.unsubscribe_role_id.is_some() {
            "Change unsubscribe role"
        } else {
            "Optional: Select role for unsubscribe permission"
        });

        let container = CreateComponent::Container(CreateContainer::new(vec![
            CreateContainerComponent::TextDisplay(CreateTextDisplay::new(status_text)),
            CreateContainerComponent::ActionRow(CreateActionRow::SelectMenu(enabled_select)),
            CreateContainerComponent::TextDisplay(CreateTextDisplay::new(channel_text)),
            CreateContainerComponent::ActionRow(CreateActionRow::SelectMenu(channel_select)),
            CreateContainerComponent::TextDisplay(CreateTextDisplay::new(sub_role_text)),
            CreateContainerComponent::ActionRow(CreateActionRow::SelectMenu(sub_role_select)),
            CreateContainerComponent::TextDisplay(CreateTextDisplay::new(unsub_role_text)),
            CreateContainerComponent::ActionRow(CreateActionRow::SelectMenu(unsub_role_select)),
        ]));

        vec![container]
    }

    fn parse_role_id(id: Option<&String>) -> Vec<RoleId> {
        id.and_then(|id| RoleId::from_str(id).ok())
            .into_iter()
            .collect()
    }

    fn parse_channel_id(id: Option<&String>) -> Vec<GenericChannelId> {
        id.and_then(|id| ChannelId::from_str(id).ok().map(GenericChannelId::from))
            .into_iter()
            .collect()
    }
}

pub struct SubscriptionsListView<'a> {
    navigation: PageNavigationView<'a>,
}

impl<'a> SubscriptionsListView<'a> {
    pub fn new(navigation: PageNavigationView<'a>) -> Self {
        Self { navigation }
    }

    pub fn navigation(&mut self) -> &mut PageNavigationView<'a> {
        &mut self.navigation
    }

    pub fn create_reply(&self, subscriptions: Vec<Subscription>) -> CreateReply<'_> {
        CreateReply::new()
            .flags(MessageFlags::IS_COMPONENTS_V2)
            .components(self.create_page(subscriptions))
    }

    pub fn create_page(&self, subscriptions: Vec<Subscription>) -> Vec<CreateComponent<'a>> {
        if subscriptions.is_empty() {
            return Self::create_empty();
        }

        let sections: Vec<CreateContainerComponent> = subscriptions
            .into_iter()
            .map(Self::create_subscription_section)
            .collect();

        let container = CreateComponent::Container(CreateContainer::new(sections));
        self.navigation.append_buttons_if_multipage(vec![container])
    }

    fn create_empty() -> Vec<CreateComponent<'a>> {
        vec![CreateComponent::Container(CreateContainer::new(vec![
            CreateContainerComponent::TextDisplay(CreateTextDisplay::new(
                "You have no subscriptions.",
            )),
        ]))]
    }

    fn create_subscription_section(sub: Subscription) -> CreateContainerComponent<'a> {
        let text = if let Some(latest) = sub.feed_latest {
            CreateTextDisplay::new(format!(
                "### {}\n\n- **Last version**: {}\n- **Last updated**: <t:{}>\n- **Source**: <{}>",
                sub.feed.name,
                latest.description,
                latest.published.timestamp(),
                sub.feed.source_url
            ))
        } else {
            CreateTextDisplay::new(format!(
                "### {}\n\n> No latest version found.\n- **Source**: <{}>",
                sub.feed.name, sub.feed.source_url
            ))
        };

        let thumbnail = CreateThumbnail::new(CreateUnfurledMediaItem::new(sub.feed.cover_url));

        CreateContainerComponent::Section(CreateSection::new(
            vec![CreateSectionComponent::TextDisplay(text)],
            CreateSectionAccessory::Thumbnail(thumbnail),
        ))
    }
}

pub struct SubscriptionBatchView;

impl SubscriptionBatchView {
    pub fn create(states: &[String], is_final: bool) -> CreateReply<'_> {
        let text_components: Vec<CreateContainerComponent> = states
            .iter()
            .map(|s| CreateContainerComponent::TextDisplay(CreateTextDisplay::new(s.clone())))
            .collect();

        let mut components = vec![CreateComponent::Container(CreateContainer::new(
            text_components,
        ))];

        if is_final {
            let nav_button = CreateButton::new("view_subscriptions")
                .label("View Subscriptions")
                .style(ButtonStyle::Secondary);

            components.push(CreateComponent::ActionRow(CreateActionRow::Buttons(
                vec![nav_button].into(),
            )));
        }

        CreateReply::new()
            .flags(MessageFlags::IS_COMPONENTS_V2)
            .components(components)
    }
}
