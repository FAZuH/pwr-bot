//! Feed settings subcommand.

use std::str::FromStr;
use std::time::Duration;

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
    Coordinator::new(ctx)
        .run(NavigationResult::SettingsFeeds)
        .await?;
    Ok(())
}

controller! { pub struct FeedSettingsController<'a> {} }

#[async_trait::async_trait]
impl Controller for FeedSettingsController<'_> {
    async fn run(&mut self, coordinator: std::sync::Arc<Coordinator<'_>>) -> Result<(), Error> {
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
    async fn handle(
        &mut self,
        ctx: ViewContext<'_, SettingsFeedAction>,
    ) -> Result<ViewCommand, Error> {
        match ctx.action() {
            SettingsFeedAction::Enabled => {
                FeedSettingsUpdate::update(FeedSettingsMsg::ToggleEnabled, &mut self.model);
                self.settings.feeds.enabled = self.model.enabled;
                Ok(ViewCommand::Render)
            }
            SettingsFeedAction::Channel => {
                let channel_id = if let ViewEvent::Component(ref interaction) = ctx.event
                    && let ComponentInteractionDataKind::ChannelSelect { values } =
                        &interaction.data.kind
                {
                    values.first().map(|id| id.to_string())
                } else {
                    None
                };
                FeedSettingsUpdate::update(
                    FeedSettingsMsg::SetChannel(channel_id),
                    &mut self.model,
                );
                self.settings.feeds.channel_id = self.model.channel_id.clone();
                Ok(ViewCommand::Render)
            }
            SettingsFeedAction::SubRole => {
                let role_id = if let ViewEvent::Component(ref interaction) = ctx.event
                    && let ComponentInteractionDataKind::RoleSelect { values } =
                        &interaction.data.kind
                {
                    values.first().map(|v| v.to_string())
                } else {
                    None
                };
                FeedSettingsUpdate::update(FeedSettingsMsg::SetSubRole(role_id), &mut self.model);
                self.settings.feeds.subscribe_role_id = self.model.subscribe_role_id.clone();
                Ok(ViewCommand::Render)
            }
            SettingsFeedAction::UnsubRole => {
                let role_id = if let ViewEvent::Component(ref interaction) = ctx.event
                    && let ComponentInteractionDataKind::RoleSelect { values } =
                        &interaction.data.kind
                {
                    values.first().map(|v| v.to_string())
                } else {
                    None
                };
                FeedSettingsUpdate::update(FeedSettingsMsg::SetUnsubRole(role_id), &mut self.model);
                self.settings.feeds.unsubscribe_role_id = self.model.unsubscribe_role_id.clone();
                Ok(ViewCommand::Render)
            }
            SettingsFeedAction::Back => {
                ctx.coordinator.navigate(NavigationResult::SettingsMain);
                Ok(ViewCommand::Exit)
            }
            SettingsFeedAction::About => {
                ctx.coordinator.navigate(NavigationResult::SettingsAbout);
                Ok(ViewCommand::Exit)
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

        let enabled_button = registry
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

        let channel_select = registry
            .register(SettingsFeedAction::Channel)
            .as_select(CreateSelectMenuKind::Channel {
                channel_types: Some(vec![ChannelType::Text, ChannelType::News].into()),
                default_channels: Some(
                    Self::parse_channel_id(self.model.channel_id.as_ref()).into(),
                ),
            })
            .placeholder(if self.model.channel_id.is_some() {
                "Change notification channel"
            } else {
                "⚠️ Required: Select a notification channel"
            });

        let sub_role_text = "### Subscribe Permission\n\n> 🛈  Who can add new feeds to this server. Leave empty to allow users with \"Manage Server\" permission.";
        let sub_role_select = registry
            .register(SettingsFeedAction::SubRole)
            .as_select(CreateSelectMenuKind::Role {
                default_roles: Some(
                    Self::parse_role_id(self.model.subscribe_role_id.as_ref()).into(),
                ),
            })
            .min_values(0)
            .placeholder(if self.model.subscribe_role_id.is_some() {
                "Change subscribe role"
            } else {
                "Optional: Select role for subscribe permission"
            });

        let unsub_role_text = "### Unsubscribe Permission\n\n> 🛈  Who can remove feeds from this server. Leave empty to allow users with \"Manage Server\" permission.";
        let unsub_role_select = registry
            .register(SettingsFeedAction::UnsubRole)
            .as_select(CreateSelectMenuKind::Role {
                default_roles: Some(
                    Self::parse_role_id(self.model.unsubscribe_role_id.as_ref()).into(),
                ),
            })
            .min_values(0)
            .placeholder(if self.model.unsubscribe_role_id.is_some() {
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

        let back_button = registry
            .register(SettingsFeedAction::Back)
            .as_button()
            .style(ButtonStyle::Secondary);
        let about_button = registry
            .register(SettingsFeedAction::About)
            .as_button()
            .style(ButtonStyle::Secondary);

        let nav_buttons = CreateComponent::ActionRow(CreateActionRow::Buttons(
            vec![back_button, about_button].into(),
        ));

        vec![container, nav_buttons].into()
    }
}
