/// Cog to manage feed subscriptions
use std::fmt::Display;
use std::str::FromStr;
use std::time::Duration;
use std::time::Instant;

use anyhow::Result;
use poise::ChoiceParameter;
use poise::CreateReply;
use poise::ReplyHandle;
use poise::serenity_prelude::AutocompleteChoice;
use poise::serenity_prelude::CreateAutocompleteResponse;
use serenity::all::ChannelId;
use serenity::all::ChannelType;
use serenity::all::ComponentInteractionCollector;
use serenity::all::ComponentInteractionDataKind;
use serenity::all::CreateActionRow;
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
use serenity::all::GuildId;
use serenity::all::MessageFlags;
use serenity::all::RoleId;
use serenity::all::UserId;

use crate::bot::cog::Context;
use crate::bot::cog::Error;
use crate::bot::checks::check_guild_permissions;
use crate::bot::components::PageNavigationComponent;
use crate::bot::components::Pagination;
use crate::bot::error::BotError;
use crate::database::model::ServerSettings;
use crate::database::model::SubscriberModel;
use crate::database::model::SubscriberType;
use crate::service::feed_subscription_service::SubscribeResult;
use crate::service::feed_subscription_service::SubscriberTarget;
use crate::service::feed_subscription_service::UnsubscribeResult;

#[derive(ChoiceParameter)]
enum SendInto {
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

impl Display for SubscribeResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let msg = match self {
            SubscribeResult::Success { feed } => format!(
                "✅ **Successfully** subscribed to [{}](<{}>)",
                feed.name, feed.url
            ),
            SubscribeResult::AlreadySubscribed { feed } => format!(
                "❌ You are **already subscribed** to [{}](<{}>)",
                feed.name, feed.url
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
                feed.name, feed.url
            ),
            UnsubscribeResult::AlreadyUnsubscribed { feed } => format!(
                "❌ You are **not subscribed** to [{}](<{}>)",
                feed.name, feed.url
            ),
            UnsubscribeResult::NoneSubscribed { url } => {
                format!("❌ You are **not subscribed** to <{}>", url)
            }
        };
        write!(f, "{}", msg)
    }
}

pub struct FeedsCog;

impl FeedsCog {
    /// Configure server feed settings
    #[poise::command(
        slash_command,
        guild_only,
        default_member_permissions = "ADMINISTRATOR | MANAGE_GUILD"
    )]
    pub async fn settings(ctx: Context<'_>) -> Result<(), Error> {
        use serenity::futures::StreamExt;
        ctx.defer().await?;
        let guild_id = ctx.guild_id().ok_or(BotError::GuildOnlyCommand)?.get();

        let mut settings = ctx
            .data()
            .feed_subscription_service
            .get_server_settings(guild_id)
            .await?;

        let msg_handle = ctx.send(FeedsCog::create_settings_reply(&settings)).await?;

        let msg = msg_handle.message().await?.into_owned();
        let author_id = ctx.author().id;

        let mut collector = ComponentInteractionCollector::new(ctx.serenity_context())
            .message_id(msg.id)
            .author_id(author_id)
            .timeout(Duration::from_secs(120))
            .stream();

        while let Some(interaction) = collector.next().await {
            // Check permissions for each interaction
            if let Err(e) = check_guild_permissions(ctx, &None).await {
                interaction
                    .create_response(
                        ctx.http(),
                        poise::serenity_prelude::CreateInteractionResponse::Message(
                            poise::serenity_prelude::CreateInteractionResponseMessage::new()
                                .content(format!("❌ {}", e))
                                .flags(MessageFlags::EPHEMERAL),
                        ),
                    )
                    .await?;
                continue;
            }

            let mut should_update = true;

            match &interaction.data.kind {
                ComponentInteractionDataKind::StringSelect { values }
                    if interaction.data.custom_id == "server_settings_enabled" =>
                {
                    if let Some(value) = values.first() {
                        settings.enabled = Some(value == "true");
                    }
                }
                ComponentInteractionDataKind::ChannelSelect { values }
                    if interaction.data.custom_id == "server_settings_channel" =>
                {
                    settings.channel_id = values.first().map(|id| id.to_string());
                }
                ComponentInteractionDataKind::RoleSelect { values }
                    if interaction.data.custom_id == "server_settings_sub_role" =>
                {
                    settings.subscribe_role_id = if values.is_empty() {
                        None
                    } else {
                        values.first().map(|id| id.to_string())
                    };
                }
                ComponentInteractionDataKind::RoleSelect { values }
                    if interaction.data.custom_id == "server_settings_unsub_role" =>
                {
                    settings.unsubscribe_role_id = if values.is_empty() {
                        None
                    } else {
                        values.first().map(|id| id.to_string())
                    };
                }
                _ => {
                    should_update = false;
                }
            }

            if should_update {
                ctx.data()
                    .feed_subscription_service
                    .update_server_settings(guild_id, settings.clone())
                    .await?;
            }

            interaction
                .create_response(
                    ctx.http(),
                    poise::serenity_prelude::CreateInteractionResponse::UpdateMessage(
                        poise::serenity_prelude::CreateInteractionResponseMessage::new()
                            .components(FeedsCog::create_settings_components(&settings)),
                    ),
                )
                .await?;
        }

        Ok(())
    }

    fn create_settings_reply(settings: &ServerSettings) -> CreateReply<'_> {
        CreateReply::new()
            .flags(MessageFlags::IS_COMPONENTS_V2)
            .components(FeedsCog::create_settings_components(settings))
    }

    fn create_settings_components(settings: &ServerSettings) -> Vec<CreateComponent<'_>> {
        let parse_role_id = |id: &Option<String>| {
            id.as_ref()
                .and_then(|id| RoleId::from_str(id).ok())
                .into_iter()
                .collect::<Vec<_>>()
        };
        let parse_channel_id = |id: &Option<String>| {
            id.as_ref()
                .and_then(|id| ChannelId::from_str(id).ok().map(GenericChannelId::from))
                .into_iter()
                .collect::<Vec<_>>()
        };
        let is_enabled = settings.enabled.unwrap_or(true);

        // Status section
        let status_text = format!(
            "## Server Feed Settings\n\n> 🛈  {}",
            if is_enabled {
                format!(
                    "Feed notifications are currently active. Notifications will be sent to <#{}>",
                    settings.channel_id.clone().unwrap_or("Unknown".to_string())
                )
            } else {
                "Feed notifications are currently paused. No notifications will be sent until re-enabled.".to_string()
            }
        );
        let enabled_select = CreateSelectMenu::new(
            "server_settings_enabled",
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

        // Channel section
        let channel_text =
            "### Notification Channel\n\n> 🛈  Choose where feed updates will be posted.";
        let channel_select = CreateSelectMenu::new(
            "server_settings_channel",
            CreateSelectMenuKind::Channel {
                channel_types: Some(vec![ChannelType::Text, ChannelType::News].into()),
                default_channels: Some(parse_channel_id(&settings.channel_id).into()),
            },
        )
        .placeholder(if settings.channel_id.is_some() {
            "Change notification channel"
        } else {
            "⚠️ Required: Select a notification channel"
        });

        // Permissions section
        let sub_role_text = "### Subscribe Permission\n\n> 🛈  Who can add new feeds to this server. Leave empty to allow users with \"Manage Server\" permission.";
        let sub_role_select = CreateSelectMenu::new(
            "server_settings_sub_role",
            CreateSelectMenuKind::Role {
                default_roles: Some(parse_role_id(&settings.subscribe_role_id).into()),
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
            "server_settings_unsub_role",
            CreateSelectMenuKind::Role {
                default_roles: Some(parse_role_id(&settings.unsubscribe_role_id).into()),
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

    /// Subscribe to a feed
    #[poise::command(slash_command)]
    pub async fn subscribe(
        ctx: Context<'_>,
        #[description = "Link(s) of the feeds. Separate links with commas (,)"]
        #[autocomplete = "Self::autocomplete_supported_feeds"]
        links: String,
        #[description = "Where to send the notifications. Default to your DM"] send_into: Option<
            SendInto,
        >,
    ) -> Result<(), Error> {
        ctx.defer().await?;

        let send_into = send_into.unwrap_or(SendInto::DM);

        if let SendInto::Server = send_into {
            let guild_id = ctx.guild_id().ok_or_else(|| {
                BotError::ConfigurationError("Command must be run in a server.".to_string())
            })?;
            let settings = ctx
                .data()
                .feed_subscription_service
                .get_server_settings(guild_id.get())
                .await?;
            if settings.channel_id.is_none() {
                return Err(BotError::ConfigurationError(
                    "Server feed settings are not configured. A server admin must run `/settings` to configure a notification channel first.".to_string(),
                )
                .into());
            }
            check_guild_permissions(ctx, &settings.subscribe_role_id).await?;
        }

        let urls_split: Vec<&str> = links.split(',').map(|s| s.trim()).collect();
        FeedsCog::validate_urls(&urls_split)?;

        let subscriber_type = SubscriberType::from(&send_into);
        let target_id = FeedsCog::get_target_id(ctx, &send_into)?;
        let target = SubscriberTarget {
            subscriber_type,
            target_id: target_id.clone(),
        };
        let subscriber = ctx
            .data()
            .feed_subscription_service
            .get_or_create_subscriber(&target)
            .await?;

        let mut states: Vec<String> = vec!["⏳ ﻿ Processing...".to_string(); urls_split.len()];

        let interval = Duration::from_secs(2);
        let mut last_send = Instant::now();

        let mut reply: Option<ReplyHandle<'_>> = None;

        // NOTE: Can be done concurrently
        for (i, url) in urls_split.iter().enumerate() {
            let sub_result = ctx
                .data()
                .feed_subscription_service
                .subscribe(url, &subscriber)
                .await;

            states[i] = sub_result.map_or_else(|e| format!("❌ {e}"), |res| res.to_string());

            let containers: Vec<CreateContainerComponent> = (0..urls_split.len())
                .map(|i| {
                    CreateContainerComponent::TextDisplay(CreateTextDisplay::new(states[i].clone()))
                })
                .collect();
            let components = vec![CreateComponent::Container(CreateContainer::new(containers))];
            let resp = CreateReply::new()
                .flags(MessageFlags::IS_COMPONENTS_V2)
                .components(components);

            if last_send.elapsed() > interval || i + 1 == urls_split.len() {
                match reply {
                    None => {
                        reply = Some(ctx.send(resp).await?);
                    }
                    Some(ref reply) => {
                        reply.edit(ctx, resp).await?;
                    }
                }
                last_send = Instant::now();
            }
        }
        Ok(())
    }

    /// Unsubscribe from a feed
    #[poise::command(slash_command)]
    pub async fn unsubscribe(
        ctx: Context<'_>,
        #[description = "Link(s) of the feeds. Separate links with commas (,)"]
        #[autocomplete = "Self::autocomplete_subscriptions"]
        links: String,
        #[description = "Where notifications were being sent. Default to DM"] send_into: Option<
            SendInto,
        >,
    ) -> Result<(), Error> {
        ctx.defer().await?;

        let send_into = send_into.unwrap_or(SendInto::DM);

        if let SendInto::Server = send_into {
            let guild_id = ctx.guild_id().ok_or_else(|| {
                BotError::ConfigurationError("Command must be run in a server.".to_string())
            })?;
            let settings = ctx
                .data()
                .feed_subscription_service
                .get_server_settings(guild_id.get())
                .await?;
            if settings.channel_id.is_none() {
                return Err(BotError::ConfigurationError(
                    "Server feed settings are not configured. An administrator must run `/settings` to configure a notification channel first.".to_string(),
                )
                .into());
            }
            check_guild_permissions(ctx, &settings.unsubscribe_role_id).await?;
        }

        let urls_split: Vec<&str> = links.split(',').map(|s| s.trim()).collect();
        FeedsCog::validate_urls(&urls_split)?;

        let subscriber_type = SubscriberType::from(&send_into);
        let target_id = FeedsCog::get_target_id(ctx, &send_into)?;
        let target = SubscriberTarget {
            subscriber_type,
            target_id: target_id.clone(),
        };
        let subscriber = ctx
            .data()
            .feed_subscription_service
            .get_or_create_subscriber(&target)
            .await?;

        let mut states: Vec<String> = vec!["⏳ ﻿ Processing...".to_string(); urls_split.len()];

        let interval = Duration::from_secs(2);
        let mut last_send = Instant::now();

        let mut reply: Option<ReplyHandle<'_>> = None;

        // NOTE: Can be done concurrently
        for (i, url) in urls_split.iter().enumerate() {
            let unsub_result = ctx
                .data()
                .feed_subscription_service
                .unsubscribe(url, &subscriber)
                .await;

            states[i] = unsub_result.map_or_else(|e| format!("❌ {e}"), |res| res.to_string());

            let containers: Vec<CreateContainerComponent> = (0..urls_split.len())
                .map(|i| {
                    CreateContainerComponent::TextDisplay(CreateTextDisplay::new(states[i].clone()))
                })
                .collect();
            let components = vec![CreateComponent::Container(CreateContainer::new(containers))];
            let resp = CreateReply::new()
                .flags(MessageFlags::IS_COMPONENTS_V2)
                .components(components);

            if last_send.elapsed() > interval || i + 1 == urls_split.len() {
                match reply {
                    None => {
                        reply = Some(ctx.send(resp).await?);
                    }
                    Some(ref reply) => {
                        reply.edit(ctx, resp).await?;
                    }
                }
                last_send = Instant::now();
            }
        }
        Ok(())
    }

    /// List all your feed subscriptions
    #[poise::command(slash_command)]
    pub async fn subscriptions(
        ctx: Context<'_>,
        #[description = "Where the notifications are being sent. Default to DM"] sent_into: Option<
            SendInto,
        >,
    ) -> Result<(), Error> {
        ctx.defer().await?;
        let sent_into = sent_into.unwrap_or(SendInto::DM);

        // Get subscriber
        let target_id = FeedsCog::get_target_id(ctx, &sent_into)?;
        let subscriber_type = SubscriberType::from(&sent_into);
        let target = SubscriberTarget {
            subscriber_type,
            target_id,
        };
        let subscriber = ctx
            .data()
            .feed_subscription_service
            .get_or_create_subscriber(&target)
            .await?;

        // Get subscriber's subscription count
        let per_page = 10;
        let items = ctx
            .data()
            .feed_subscription_service
            .get_subscription_count(&subscriber)
            .await?;

        // Create navigation component
        let pages = items.div_ceil(per_page);
        let mut navigation =
            PageNavigationComponent::new(&ctx, Pagination::new(pages, per_page, 1));

        // Run feedback loop until timeout
        let reply = ctx
            .send(
                CreateReply::new()
                    .flags(MessageFlags::IS_COMPONENTS_V2)
                    .components(FeedsCog::create_page(&ctx, &subscriber, &navigation).await?),
            )
            .await?;

        while navigation.listen(Duration::from_secs(60)).await {
            reply
                .edit(
                    ctx,
                    CreateReply::new()
                        .flags(MessageFlags::IS_COMPONENTS_V2)
                        .components(FeedsCog::create_page(&ctx, &subscriber, &navigation).await?),
                )
                .await?;
        }

        Ok(())
    }

    async fn create_page<'a>(
        ctx: &Context<'_>,
        subscriber: &SubscriberModel,
        navigation: &'a PageNavigationComponent<'_>,
    ) -> anyhow::Result<Vec<CreateComponent<'a>>> {
        let subscriptions = ctx
            .data()
            .feed_subscription_service
            .list_paginated_subscriptions(
                subscriber,
                navigation.pagination.current_page,
                navigation.pagination.per_page,
            )
            .await?;

        if subscriptions.is_empty() {
            let text = CreateTextDisplay::new("You have no subscriptions.");
            let empty_container = CreateComponent::Container(CreateContainer::new(vec![
                CreateContainerComponent::TextDisplay(text),
            ]));
            return Ok(vec![empty_container]);
        }

        let mut container_components = vec![];
        for sub in subscriptions {
            let text = if let Some(latest) = sub.feed_latest {
                CreateTextDisplay::new(format!(
                    "### {}\n\n- **Last version**: {}\n- **Last updated**: <t:{}>\n- **Source**: <{}>",
                    sub.feed.name,
                    latest.description,
                    latest.published.timestamp(),
                    sub.feed.url
                ))
            } else {
                // Note: You need to provide the feed name and URL for this case too
                CreateTextDisplay::new(format!(
                    "### {}\n\n> No latest version found.\n- **Source**: <{}>",
                    sub.feed.name, sub.feed.url
                ))
            };
            let thumbnail = CreateThumbnail::new(CreateUnfurledMediaItem::new(sub.feed.cover_url));

            container_components.push(CreateContainerComponent::Section(CreateSection::new(
                vec![CreateSectionComponent::TextDisplay(text)],
                CreateSectionAccessory::Thumbnail(thumbnail),
            )))
        }

        let container = CreateComponent::Container(CreateContainer::new(container_components));
        if navigation.pagination.pages == 1 {
            Ok(vec![container])
        } else {
            let buttons = navigation.create_buttons();
            Ok(vec![container, buttons])
        }
    }

    async fn autocomplete_supported_feeds<'a>(
        ctx: Context<'_>,
        partial: &str,
    ) -> CreateAutocompleteResponse<'a> {
        let mut choices = vec![AutocompleteChoice::new("Supported feeds are:", "foo")];
        let feeds = ctx.data().feeds.get_all_feeds();

        for feed in feeds {
            let info = &feed.get_base().info;
            let name = format!("{} ({})", info.name, info.api_domain);
            if partial.is_empty()
                || name.to_lowercase().contains(&partial.to_lowercase())
                || info.api_domain.contains(&partial.to_lowercase())
            {
                choices.push(AutocompleteChoice::new(name, info.api_domain.clone()));
            }
        }

        choices.truncate(25);
        CreateAutocompleteResponse::new().set_choices(choices)
    }

    async fn autocomplete_subscriptions<'a>(
        ctx: Context<'_>,
        partial: &str,
    ) -> CreateAutocompleteResponse<'a> {
        if partial.trim().is_empty() {
            return CreateAutocompleteResponse::new().set_choices(vec![AutocompleteChoice::from(
                "Start typing to see suggestions",
            )]);
        }

        // Get subscriber
        let user_target = SubscriberTarget {
            target_id: ctx.author().id.to_string(),
            subscriber_type: SubscriberType::Dm,
        };
        let guild_target = ctx.guild_id().map(|res| SubscriberTarget {
            target_id: res.to_string(),
            subscriber_type: SubscriberType::Guild,
        });
        let user_subscriber = ctx
            .data()
            .feed_subscription_service
            .get_or_create_subscriber(&user_target)
            .await
            .ok();
        let guild_subscriber = match guild_target {
            Some(guild_target) => ctx
                .data()
                .feed_subscription_service
                .get_or_create_subscriber(&guild_target)
                .await
                .ok(),
            None => None,
        };
        if user_subscriber.is_none() && guild_subscriber.is_none() {
            return CreateAutocompleteResponse::new();
        }

        // Get subscribed feeds
        let mut user_feeds = match user_subscriber {
            Some(user_subscriber) => ctx
                .data()
                .feed_subscription_service
                .search_subcriptions(&user_subscriber, partial)
                .await
                .unwrap_or(vec![]),
            None => vec![],
        };
        let mut guild_feeds = match guild_subscriber {
            Some(guild_subscriber) => ctx
                .data()
                .feed_subscription_service
                .search_subcriptions(&guild_subscriber, partial)
                .await
                .unwrap_or(vec![]),
            None => vec![],
        };
        if ctx.guild_id().is_none() && user_feeds.is_empty() {
            return CreateAutocompleteResponse::new().set_choices(vec![AutocompleteChoice::from(
                "You have no subscriptions yet. Subscribe first with `/subscribe` command",
            )]);
        }

        // Combine the feeds
        for f in &mut user_feeds {
            f.name.insert_str(0, "(DM) ");
        }
        for f in &mut guild_feeds {
            f.name.insert_str(0, "(Server) ");
        }
        // NOTE: search_subcriptions already returns Vec<FeedModel> sorted by FeedModel.name, so we
        // don't need to sort it here.
        user_feeds.append(&mut guild_feeds);
        let feeds = user_feeds;

        // Map the feeds into AutocompleteChoices
        let mut choices = feeds
            .into_iter()
            .map(|feed| AutocompleteChoice::new(feed.name, feed.url))
            .collect::<Vec<_>>();

        // Discord autocomplete limit
        choices.truncate(25);
        CreateAutocompleteResponse::new().set_choices(choices)
    }

    fn get_target_id(ctx: Context<'_>, send_into: &SendInto) -> Result<String, BotError> {
        Self::get_target_id_inner(ctx.guild_id(), ctx.author().id, send_into)
    }

    fn get_target_id_inner(
        guild_id: Option<GuildId>,
        author_id: UserId,
        send_into: &SendInto,
    ) -> Result<String, BotError> {
        match send_into {
            SendInto::Server => {
                let guild_id = guild_id.ok_or_else(|| BotError::InvalidCommandArgument {
                    parameter: send_into.name().to_string(),
                    reason: "You have to be in a server to do this command with send_into: server"
                        .to_string(),
                })?;
                Ok(guild_id.to_string())
            }
            SendInto::DM => Ok(author_id.to_string()),
        }
    }

    fn validate_urls(urls: &[&str]) -> Result<(), BotError> {
        if urls.len() > 10 {
            return Err(BotError::InvalidCommandArgument {
                parameter: "links".to_string(),
                reason: "Too many links provided. Please provide no more than 10 links at a time."
                    .to_string(),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use serenity::all::GuildId;
    use serenity::all::UserId;

    use super::*;

    #[test]
    fn test_get_target_id_dm_returns_author_id() {
        let result = FeedsCog::get_target_id_inner(
            Some(GuildId::new(999)),
            UserId::new(12345),
            &SendInto::DM,
        );
        assert_eq!(result.unwrap(), "12345");
    }

    #[test]
    fn test_get_target_id_server_returns_guild_id() {
        let result = FeedsCog::get_target_id_inner(
            Some(GuildId::new(999)),
            UserId::new(12345),
            &SendInto::Server,
        );
        assert_eq!(result.unwrap(), "999");
    }

    #[test]
    fn test_get_target_id_server_without_guild_fails() {
        let result = FeedsCog::get_target_id_inner(None, UserId::new(12345), &SendInto::Server);
        assert!(result.is_err());
        match result.unwrap_err() {
            BotError::InvalidCommandArgument { parameter, reason } => {
                assert_eq!(parameter, "Server");
                assert!(reason.contains("have to be in a server"));
            }
            _ => panic!("Expected InvalidCommandArgument error"),
        }
    }

    #[test]
    fn test_send_into_to_subscriber_type() {
        assert!(matches!(
            SubscriberType::from(&SendInto::DM),
            SubscriberType::Dm
        ));
        assert!(matches!(
            SubscriberType::from(&SendInto::Server),
            SubscriberType::Guild
        ));
    }

    #[test]
    fn test_send_into_display() {
        assert_eq!(SendInto::DM.to_string(), "dm");
        assert_eq!(SendInto::Server.to_string(), "server");
    }

    #[test]
    fn test_validate_urls_accepts_valid_count() {
        let urls = vec!["url1", "url2", "url3"];
        assert!(FeedsCog::validate_urls(&urls).is_ok());
    }

    #[test]
    fn test_validate_urls_rejects_too_many() {
        let urls = vec!["url"; 11];
        let result = FeedsCog::validate_urls(&urls);
        assert!(result.is_err());
        match result.unwrap_err() {
            BotError::InvalidCommandArgument { parameter, reason } => {
                assert_eq!(parameter, "links");
                assert!(reason.contains("no more than 10"));
            }
            _ => panic!("Expected InvalidCommandArgument error"),
        }
    }

    #[test]
    fn test_validate_urls_accepts_exactly_ten() {
        let urls = vec!["url"; 10];
        assert!(FeedsCog::validate_urls(&urls).is_ok());
    }
}
