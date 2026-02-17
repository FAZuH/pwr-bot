//! Views for feed-related commands.

use std::collections::HashSet;
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
use serenity::all::CreateSelectMenuKind;
use serenity::all::CreateTextDisplay;
use serenity::all::CreateThumbnail;
use serenity::all::CreateUnfurledMediaItem;
use serenity::all::GenericChannelId;
use serenity::all::RoleId;

use crate::action_enum;
use crate::action_extends;
use crate::bot::commands::Context;
use crate::bot::commands::Error;
use crate::bot::views::Action;
use crate::bot::views::ChildViewResolver;
use crate::bot::views::InteractiveView;
use crate::bot::views::RenderExt;
use crate::bot::views::ResponseKind;
use crate::bot::views::ResponseView;
use crate::bot::views::View;
use crate::bot::views::pagination::PaginationAction;
use crate::bot::views::pagination::PaginationView;
use crate::database::model::ServerSettings;
use crate::service::feed_subscription_service::Subscription;
use crate::view_core;

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

action_enum! {
    FeedSubscriptionBatchAction {
        #[label = "View Subscriptions"]
        ViewSubscriptions,
    }
}

view_core! {
    timeout = Duration::from_secs(120),
    /// View for configuring server feed settings.
    pub struct SettingsFeedView<'a, SettingsFeedAction> {
        pub settings: &'a mut ServerSettings,
    }
}

impl<'a> SettingsFeedView<'a> {
    /// Creates a new settings view with the given settings reference.
    pub fn new(ctx: &'a Context<'a>, settings: &'a mut ServerSettings) -> Self {
        Self {
            settings,
            core: Self::create_core(ctx),
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

impl<'a> ResponseView<'a> for SettingsFeedView<'a> {
    fn create_response<'b>(&mut self) -> ResponseKind<'b> {
        let is_enabled = self.settings.feeds.enabled.unwrap_or(true);

        let status_text = format!(
            "-# **Settings > Feeds**\n## Feed Subscription Settings\n\n> 🛈  {}",
            if is_enabled {
                match &self.settings.feeds.channel_id {
                    Some(id) => format!("Feed notifications are currently **active**. Notifications will be sent to <#{id}>"),
                    None => "Feed notifications are currently **active**, but notification channel is not set.".to_string(),
                }
            } else {
                "Feed notifications are currently **paused**. No notifications will be sent until it is re-enabled.".to_string()
            }
        );

        let enabled_button = self
            .register(SettingsFeedAction::Enabled)
            .as_button()
            .label(if is_enabled { "Disable" } else { "Enable" })
            .style(if is_enabled {
                ButtonStyle::Danger
            } else {
                ButtonStyle::Success
            });

        let channel_text =
            "### Notification Channel\n\n> 🛈  Choose where feed updates will be posted.";

        let channel_select = self
            .register(SettingsFeedAction::Channel)
            .as_select(CreateSelectMenuKind::Channel {
                channel_types: Some(vec![ChannelType::Text, ChannelType::News].into()),
                default_channels: Some(
                    Self::parse_channel_id(self.settings.feeds.channel_id.as_ref()).into(),
                ),
            })
            .placeholder(if self.settings.feeds.channel_id.is_some() {
                "Change notification channel"
            } else {
                "⚠️ Required: Select a notification channel"
            });

        let sub_role_text = "### Subscribe Permission\n\n> 🛈  Who can add new feeds to this server. Leave empty to allow users with \"Manage Server\" permission.";
        let sub_role_select = self
            .register(SettingsFeedAction::SubRole)
            .as_select(CreateSelectMenuKind::Role {
                default_roles: Some(
                    Self::parse_role_id(self.settings.feeds.subscribe_role_id.as_ref()).into(),
                ),
            })
            .min_values(0)
            .placeholder(if self.settings.feeds.subscribe_role_id.is_some() {
                "Change subscribe role"
            } else {
                "Optional: Select role for subscribe permission"
            });

        let unsub_role_text = "### Unsubscribe Permission\n\n> 🛈  Who can remove feeds from this server. Leave empty to allow users with \"Manage Server\" permission.";
        let unsub_role_select = self
            .register(SettingsFeedAction::UnsubRole)
            .as_select(CreateSelectMenuKind::Role {
                default_roles: Some(
                    Self::parse_role_id(self.settings.feeds.unsubscribe_role_id.as_ref()).into(),
                ),
            })
            .min_values(0)
            .placeholder(if self.settings.feeds.unsubscribe_role_id.is_some() {
                "Change unsubscribe role"
            } else {
                "Optional: Select role for unsubscribe permission"
            });

        let container = CreateComponent::Container(CreateContainer::new(vec![
            CreateContainerComponent::TextDisplay(CreateTextDisplay::new(status_text)),
            CreateContainerComponent::ActionRow(CreateActionRow::Buttons(
                vec![enabled_button].into(),
            )),
            CreateContainerComponent::TextDisplay(CreateTextDisplay::new(channel_text)),
            CreateContainerComponent::ActionRow(CreateActionRow::SelectMenu(channel_select)),
            CreateContainerComponent::TextDisplay(CreateTextDisplay::new(sub_role_text)),
            CreateContainerComponent::ActionRow(CreateActionRow::SelectMenu(sub_role_select)),
            CreateContainerComponent::TextDisplay(CreateTextDisplay::new(unsub_role_text)),
            CreateContainerComponent::ActionRow(CreateActionRow::SelectMenu(unsub_role_select)),
        ]));

        let back = self.register(SettingsFeedAction::Back);
        let about = self.register(SettingsFeedAction::About);

        let nav_buttons = CreateComponent::ActionRow(CreateActionRow::Buttons(
            vec![
                CreateButton::new(back.id)
                    .label(SettingsFeedAction::Back.label())
                    .style(ButtonStyle::Secondary),
                CreateButton::new(about.id)
                    .label(SettingsFeedAction::About.label())
                    .style(ButtonStyle::Secondary),
            ]
            .into(),
        ));

        vec![container, nav_buttons].into()
    }
}

#[async_trait::async_trait]
impl<'a> InteractiveView<'a, SettingsFeedAction> for SettingsFeedView<'a> {
    async fn handle(
        &mut self,
        action: &SettingsFeedAction,
        interaction: &ComponentInteraction,
    ) -> Option<SettingsFeedAction> {
        let data = &interaction.data;
        let settings = &mut self.settings.feeds;

        match (&data.kind, action) {
            (ComponentInteractionDataKind::Button, SettingsFeedAction::Enabled) => {
                let current = settings.enabled.unwrap_or(true);
                settings.enabled = Some(!current);
                Some(action.clone())
            }
            (
                ComponentInteractionDataKind::ChannelSelect { values },
                SettingsFeedAction::Channel,
            ) => {
                settings.channel_id = values.first().map(|id| id.to_string());
                Some(action.clone())
            }
            (ComponentInteractionDataKind::RoleSelect { values }, SettingsFeedAction::SubRole) => {
                settings.subscribe_role_id = values.first().map(|v| v.to_string());
                Some(action.clone())
            }
            (
                ComponentInteractionDataKind::RoleSelect { values },
                SettingsFeedAction::UnsubRole,
            ) => {
                settings.unsubscribe_role_id = values.first().map(|v| v.to_string());
                Some(action.clone())
            }
            (ComponentInteractionDataKind::Button, SettingsFeedAction::Back)
            | (ComponentInteractionDataKind::Button, SettingsFeedAction::About) => {
                Some(action.clone())
            }
            _ => None,
        }
    }
}

pub enum FeedListState {
    View,
    Edit,
}

view_core! {
    timeout = Duration::from_secs(120),
    pub struct FeedListView<'a, FeedListAction> {
        subscriptions: Vec<Subscription>,
        pub pagination: PaginationView<'a>,
        pub state: FeedListState,
        pub marked_unsub: HashSet<String>
    }
}

impl<'a> FeedListView<'a> {
    pub fn new(
        ctx: &'a Context<'a>,
        subscriptions: Vec<Subscription>,
        pagination: PaginationView<'a>,
    ) -> Self {
        Self {
            subscriptions,
            pagination,
            core: Self::create_core(ctx),
            state: FeedListState::View,
            marked_unsub: HashSet::new(),
        }
    }

    /// Updates the subscriptions list.
    pub fn set_subscriptions(&mut self, subscriptions: Vec<Subscription>) -> &mut Self {
        self.subscriptions = subscriptions;
        self
    }

    /// Creates an empty state view.
    fn create_empty<'b>() -> Vec<CreateComponent<'b>> {
        vec![CreateComponent::Container(CreateContainer::new(vec![
            CreateContainerComponent::TextDisplay(CreateTextDisplay::new(
                "You have no subscriptions.",
            )),
        ]))]
    }

    /// Creates a section component for a single subscription.
    fn create_subscription_section<'b>(
        &mut self,
        sub: Subscription,
    ) -> CreateContainerComponent<'b> {
        use FeedListAction::*;
        let text = if let Some(latest) = sub.feed_latest {
            format!(
                "### {}\n\n- **Last version**: {}\n- **Last updated**: <t:{}>\n- [**Source** 🗗](<{}>)",
                sub.feed.name,
                latest.description,
                latest.published.timestamp(),
                sub.feed.source_url
            )
        } else {
            format!(
                "### {}\n\n> No latest version found.\n- [**Source** 🗗](<{}>)",
                sub.feed.name, sub.feed.source_url
            )
        };

        let text_component = CreateSectionComponent::TextDisplay(CreateTextDisplay::new(text));

        let accessory = match self.state {
            FeedListState::View => CreateSectionAccessory::Thumbnail(CreateThumbnail::new(
                CreateUnfurledMediaItem::new(sub.feed.cover_url),
            )),
            FeedListState::Edit => {
                let source_url = sub.feed.source_url;
                let button = if self.marked_unsub.contains(&source_url) {
                    self.register(UndoUnsub { source_url })
                        .as_button()
                        .style(ButtonStyle::Secondary)
                } else {
                    self.register(Unsubscribe { source_url })
                        .as_button()
                        .style(ButtonStyle::Danger)
                };
                CreateSectionAccessory::Button(button)
            }
        };

        CreateContainerComponent::Section(CreateSection::new(vec![text_component], accessory))
    }

    /// Create button section of the view at the bottom.
    fn create_toggle_button<'b>(&mut self) -> CreateComponent<'b> {
        let action = match self.state {
            FeedListState::Edit => FeedListAction::View,
            FeedListState::View => FeedListAction::Edit,
        };

        let state_button = self
            .register(action)
            .as_button()
            .style(ButtonStyle::Primary);

        let mut save_button = self
            .register(FeedListAction::Save)
            .as_button()
            .style(ButtonStyle::Success);

        if self.marked_unsub.is_empty() {
            save_button = save_button.disabled(true)
        }

        let buttons = vec![state_button, save_button];

        CreateComponent::ActionRow(CreateActionRow::Buttons(buttons.into()))
    }
}

impl<'a> ResponseView<'a> for FeedListView<'a> {
    fn create_response<'b>(&mut self) -> ResponseKind<'b> {
        if self.subscriptions.is_empty() {
            return Self::create_empty().into();
        }

        let sections: Vec<CreateContainerComponent<'b>> = self
            .subscriptions
            .clone()
            .into_iter()
            .map(|sub| self.create_subscription_section(sub))
            .collect();

        let container = CreateComponent::Container(CreateContainer::new(sections));
        let mut components = vec![container];

        self.pagination.attach_if_multipage(&mut components);
        components.push(self.create_toggle_button());

        components.into()
    }
}

action_extends! { FeedListAction extends PaginationAction {
    #[label =  "✎ Edit"]
    Edit,
    #[label = "👁 View"]
    View,
    #[label = "🗑 Unsubscribe"]
    Unsubscribe { source_url: String },
    #[label = "↶ Undo"]
    UndoUnsub { source_url: String },
    Save,
    Exit,
}}

#[async_trait::async_trait]
impl<'a> InteractiveView<'a, FeedListAction> for FeedListView<'a> {
    async fn handle(
        &mut self,
        action: &FeedListAction,
        interaction: &ComponentInteraction,
    ) -> Option<FeedListAction> {
        use FeedListAction::*;
        match action {
            Base(pagination_action) => {
                let action = self
                    .pagination
                    .handle(pagination_action, interaction)
                    .await?;
                Some(Base(action))
            }
            Edit => {
                self.state = FeedListState::Edit;
                Some(action.clone())
            }
            View => {
                self.state = FeedListState::View;
                Some(action.clone())
            }
            Unsubscribe { source_url } => {
                self.marked_unsub.insert(source_url.clone());
                Some(action.clone())
            }
            UndoUnsub { source_url } => {
                self.marked_unsub.remove(source_url);
                Some(action.clone())
            }
            Exit | Save => Some(action.clone()),
        }
    }

    async fn on_timeout(&mut self) -> Result<(), Error> {
        self.pagination.disabled = true;
        self.render().await?;
        Ok(())
    }

    fn children(&mut self) -> Vec<Box<dyn ChildViewResolver<FeedListAction> + '_>> {
        vec![Self::child(&mut self.pagination, FeedListAction::Base)]
    }
}

view_core! {
    timeout = Duration::from_secs(120),
    /// View that shows the progress of a subscription batch operation.
    pub struct FeedSubscriptionBatchView<'a, FeedSubscriptionBatchAction> {
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
            core: Self::create_core(ctx),
        }
    }
}

impl<'a> ResponseView<'a> for FeedSubscriptionBatchView<'a> {
    fn create_response<'b>(&mut self) -> ResponseKind<'b> {
        let text_components: Vec<CreateContainerComponent> = self
            .states
            .iter()
            .map(|s| CreateContainerComponent::TextDisplay(CreateTextDisplay::new(s.clone())))
            .collect();

        let mut components = vec![CreateComponent::Container(CreateContainer::new(
            text_components,
        ))];

        if self.is_final {
            let nav_button = self
                .register(FeedSubscriptionBatchAction::ViewSubscriptions)
                .as_button()
                .style(ButtonStyle::Secondary);

            components.push(CreateComponent::ActionRow(CreateActionRow::Buttons(
                vec![nav_button].into(),
            )));
        }

        components.into()
    }
}

#[async_trait::async_trait]
impl<'a> InteractiveView<'a, FeedSubscriptionBatchAction> for FeedSubscriptionBatchView<'a> {
    async fn handle(
        &mut self,
        action: &FeedSubscriptionBatchAction,
        _interaction: &ComponentInteraction,
    ) -> Option<FeedSubscriptionBatchAction> {
        match action {
            FeedSubscriptionBatchAction::ViewSubscriptions => Some(action.clone()),
        }
    }
}
