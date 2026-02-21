//! Feed settings subcommand.

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
use serenity::all::CreateSelectMenuKind;
use serenity::all::CreateTextDisplay;
use serenity::all::GenericChannelId;
use serenity::all::RoleId;

use crate::action_enum;
use crate::bot::commands::Context;
use crate::bot::commands::Error;
use crate::bot::commands::settings::SettingsPage;
use crate::bot::commands::settings::run_settings;
use crate::bot::controller::Controller;
use crate::bot::controller::Coordinator;
use crate::bot::error::BotError;
use crate::bot::navigation::NavigationResult;
use crate::bot::views::Action;
use crate::bot::views::InteractiveView;
use crate::bot::views::RenderExt;
use crate::bot::views::ResponseKind;
use crate::bot::views::ResponseView;
use crate::bot::views::View;
use crate::controller;
use crate::model::ServerSettings;
use crate::view_core;

/// Configure feed settings for this server
///
/// Set up notification channels and required roles for feed subscriptions.
/// Only server administrators can use this command.
#[poise::command(
    slash_command,
    default_member_permissions = "ADMINISTRATOR | MANAGE_GUILD"
)]
pub async fn settings(ctx: Context<'_>) -> Result<(), Error> {
    run_settings(ctx, Some(SettingsPage::Feeds)).await
}

controller! { pub struct FeedSettingsController<'a> {} }

#[async_trait::async_trait]
impl<'a, S: Send + Sync + 'static> Controller<S> for FeedSettingsController<'a> {
    async fn run(
        &mut self,
        coordinator: &mut Coordinator<'_, S>,
    ) -> Result<NavigationResult, Error> {
        let ctx = *coordinator.context();
        ctx.defer().await?;
        let service = ctx.data().service.feed_subscription.clone();

        let guild_id = ctx.guild_id().ok_or(BotError::GuildOnlyCommand)?.get();

        let mut settings = service.get_server_settings(guild_id).await?;

        let mut view = SettingsFeedView::new(&ctx, &mut settings);
        view.render().await?;

        while let Some((action, _)) = view.listen_once().await? {
            if action == SettingsFeedAction::Back {
                return Ok(NavigationResult::Back);
            } else if action == SettingsFeedAction::About {
                return Ok(NavigationResult::SettingsAbout);
            }

            service
                .update_server_settings(guild_id, view.settings.clone())
                .await?;

            view.render().await?;
        }

        Ok(NavigationResult::Exit)
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

        let nav_buttons =
            CreateComponent::ActionRow(CreateActionRow::Buttons(<std::borrow::Cow<
                '_,
                [serenity::all::CreateButton<'_>],
            >>::from(vec![
                CreateButton::new(back.id)
                    .label(SettingsFeedAction::Back.label())
                    .style(ButtonStyle::Secondary),
                CreateButton::new(about.id)
                    .label(SettingsFeedAction::About.label())
                    .style(ButtonStyle::Secondary),
            ])));

        vec![container, nav_buttons].into()
    }
}

#[async_trait::async_trait]
impl<'a> InteractiveView<'a, SettingsFeedAction> for SettingsFeedView<'a> {
    async fn handle(
        &mut self,
        action: &SettingsFeedAction,
        interaction: &ComponentInteraction,
    ) -> Result<Option<SettingsFeedAction>, Error> {
        let data = &interaction.data;
        let settings = &mut self.settings.feeds;

        match (&data.kind, action) {
            (ComponentInteractionDataKind::Button, SettingsFeedAction::Enabled) => {
                let current = settings.enabled.unwrap_or(true);
                settings.enabled = Some(!current);
                Ok(Some(action.clone()))
            }
            (
                ComponentInteractionDataKind::ChannelSelect { values },
                SettingsFeedAction::Channel,
            ) => {
                settings.channel_id = values.first().map(|id| id.to_string());
                Ok(Some(action.clone()))
            }
            (ComponentInteractionDataKind::RoleSelect { values }, SettingsFeedAction::SubRole) => {
                settings.subscribe_role_id = values.first().map(|v| v.to_string());
                Ok(Some(action.clone()))
            }
            (
                ComponentInteractionDataKind::RoleSelect { values },
                SettingsFeedAction::UnsubRole,
            ) => {
                settings.unsubscribe_role_id = values.first().map(|v| v.to_string());
                Ok(Some(action.clone()))
            }
            (ComponentInteractionDataKind::Button, SettingsFeedAction::Back)
            | (ComponentInteractionDataKind::Button, SettingsFeedAction::About) => {
                Ok(Some(action.clone()))
            }
            _ => Ok(None),
        }
    }
}
