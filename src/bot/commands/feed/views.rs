//! Views for feed-related commands.

use std::str::FromStr;
use std::time::Duration;

use serenity::all::ButtonStyle;
use serenity::all::ChannelId;
use serenity::all::ChannelType;
use serenity::all::ComponentInteraction;
use serenity::all::ComponentInteractionDataKind;
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
use serenity::all::RoleId;

use crate::bot::commands::Context;
use crate::bot::views::Action;
use crate::bot::views::InteractableComponentView;
use crate::bot::views::ResponseComponentView;
use crate::custom_id_enum;
use crate::database::model::ServerSettings;
use crate::service::feed_subscription_service::Subscription;
use crate::stateful_view;

custom_id_enum!(SettingsFeedAction {
    Enabled,
    Channel,
    SubRole,
    UnsubRole,
    Back = "❮ Back",
    About = "About",
});

custom_id_enum!(FeedSubscriptionBatchAction { ViewSubscriptions });

stateful_view! {
    timeout = Duration::from_secs(120),
    /// View for configuring server feed settings.
    pub struct SettingsFeedView<'a> {
        pub settings: &'a mut ServerSettings,
    }
}

impl<'a> SettingsFeedView<'a> {
    /// Creates a new settings view with the given settings reference.
    pub fn new(ctx: &'a Context<'a>, settings: &'a mut ServerSettings) -> Self {
        Self {
            settings,
            ctx: Self::create_context(ctx),
        }
    }

    /// Updates the settings reference.
    pub fn set_settings(&mut self, settings: &'a mut ServerSettings) {
        self.settings = settings;
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
}

impl ResponseComponentView for SettingsFeedView<'_> {
    fn create_components<'a>(&self) -> Vec<CreateComponent<'a>> {
        let settings = &self.settings.feeds;
        let is_enabled = settings.enabled.unwrap_or(true);

        let status_text = format!(
            "### Server Feed Settings\n\n> 🛈  {}",
            if is_enabled {
                match &settings.channel_id {
                    Some(id) => format!("Feed notifications are currently active. Notifications will be sent to <#{id}>"),
                    None => "Feed notifications are currently active, but notification channel is not set.".to_string(),
                }
            } else {
                "Feed notifications are currently paused. No notifications will be sent until re-enabled.".to_string()
            }
        );

        let enabled_select = CreateSelectMenu::new(
            SettingsFeedAction::Enabled.custom_id(),
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
            SettingsFeedAction::Channel.custom_id(),
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
            SettingsFeedAction::SubRole.custom_id(),
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
            SettingsFeedAction::UnsubRole.custom_id(),
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

        let nav_buttons = CreateComponent::ActionRow(CreateActionRow::Buttons(
            vec![
                CreateButton::new(SettingsFeedAction::Back.custom_id())
                    .label(SettingsFeedAction::Back.label())
                    .style(ButtonStyle::Secondary),
                CreateButton::new(SettingsFeedAction::About.custom_id())
                    .label(SettingsFeedAction::About.label())
                    .style(ButtonStyle::Secondary),
            ]
            .into(),
        ));

        vec![container, nav_buttons]
    }
}

#[async_trait::async_trait]
impl<'a> InteractableComponentView<'a, SettingsFeedAction> for SettingsFeedView<'a> {
    async fn handle(&mut self, interaction: &ComponentInteraction) -> Option<SettingsFeedAction> {
        let action = SettingsFeedAction::from_str(&interaction.data.custom_id).ok()?;
        let data = &interaction.data;

        let settings = &mut self.settings.feeds;
        match (&data.kind, action) {
            (
                ComponentInteractionDataKind::StringSelect { values },
                SettingsFeedAction::Enabled,
            ) => {
                settings.enabled = values.first().map(|v| v == "true");
                Some(action)
            }
            (
                ComponentInteractionDataKind::ChannelSelect { values },
                SettingsFeedAction::Channel,
            ) => {
                settings.channel_id = values.first().map(|id| id.to_string());
                Some(action)
            }
            (ComponentInteractionDataKind::RoleSelect { values }, SettingsFeedAction::SubRole) => {
                settings.subscribe_role_id = values.first().map(|v| v.to_string());
                Some(action)
            }
            (
                ComponentInteractionDataKind::RoleSelect { values },
                SettingsFeedAction::UnsubRole,
            ) => {
                settings.unsubscribe_role_id = values.first().map(|v| v.to_string());
                Some(action)
            }
            (ComponentInteractionDataKind::Button, SettingsFeedAction::Back)
            | (ComponentInteractionDataKind::Button, SettingsFeedAction::About) => Some(action),
            _ => None,
        }
    }
}

/// View that displays a list of feed subscriptions.
pub struct FeedSubscriptionsListView {
    subscriptions: Vec<Subscription>,
}

impl FeedSubscriptionsListView {
    /// Creates a new subscriptions list view.
    pub fn new(subscriptions: Vec<Subscription>) -> Self {
        Self { subscriptions }
    }

    /// Updates the subscriptions list.
    pub fn set_subscriptions(&mut self, subscriptions: Vec<Subscription>) -> &mut Self {
        self.subscriptions = subscriptions;
        self
    }

    /// Creates an empty state view.
    fn create_empty<'a>() -> Vec<CreateComponent<'a>> {
        vec![CreateComponent::Container(CreateContainer::new(vec![
            CreateContainerComponent::TextDisplay(CreateTextDisplay::new(
                "You have no subscriptions.",
            )),
        ]))]
    }

    /// Creates a section component for a single subscription.
    fn create_subscription_section<'a>(sub: Subscription) -> CreateContainerComponent<'a> {
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

impl ResponseComponentView for FeedSubscriptionsListView {
    fn create_components<'a>(&self) -> Vec<CreateComponent<'a>> {
        if self.subscriptions.is_empty() {
            return Self::create_empty();
        }

        let sections: Vec<CreateContainerComponent<'a>> = self
            .subscriptions
            .clone()
            .into_iter()
            .map(Self::create_subscription_section)
            .collect();

        let container = CreateComponent::Container(CreateContainer::new(sections));
        vec![container]
    }
}

stateful_view! {
    timeout = Duration::from_secs(120),
    /// View that shows the progress of a subscription batch operation.
    pub struct FeedSubscriptionBatchView<'a> {
        states: Vec<String>,
        is_final: bool,
    }
}

impl<'a> FeedSubscriptionBatchView<'a> {
    /// Creates a new batch view with the given states.
    pub fn new(ctx: &'a Context<'a>, states: Vec<String>, is_final: bool) -> Self {
        Self {
            states,
            is_final,
            ctx: Self::create_context(ctx),
        }
    }
}

impl<'a> ResponseComponentView for FeedSubscriptionBatchView<'a> {
    fn create_components<'b>(&self) -> Vec<CreateComponent<'b>> {
        let text_components: Vec<CreateContainerComponent> = self
            .states
            .iter()
            .map(|s| CreateContainerComponent::TextDisplay(CreateTextDisplay::new(s.clone())))
            .collect();

        let mut components = vec![CreateComponent::Container(CreateContainer::new(
            text_components,
        ))];

        if self.is_final {
            let nav_button =
                CreateButton::new(FeedSubscriptionBatchAction::ViewSubscriptions.custom_id())
                    .label("View Subscriptions")
                    .style(ButtonStyle::Secondary);

            components.push(CreateComponent::ActionRow(CreateActionRow::Buttons(
                vec![nav_button].into(),
            )));
        }

        components
    }
}

#[async_trait::async_trait]
impl<'a> InteractableComponentView<'a, FeedSubscriptionBatchAction>
    for FeedSubscriptionBatchView<'a>
{
    async fn handle(
        &mut self,
        interaction: &ComponentInteraction,
    ) -> Option<FeedSubscriptionBatchAction> {
        FeedSubscriptionBatchAction::from_str(&interaction.data.custom_id).ok()
    }
}
