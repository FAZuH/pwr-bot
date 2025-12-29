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
use serenity::all::CreateTextDisplay;
use serenity::all::CreateThumbnail;
use serenity::all::CreateUnfurledMediaItem;
use serenity::all::GenericChannelId;
use serenity::all::MessageFlags;

use crate::bot::cog::Context;
use crate::bot::cog::Error;
use crate::bot::components::PageNavigationComponent;
use crate::bot::components::Pagination;
use crate::bot::error::BotError;
use crate::database::model::ServerSettings;
use crate::database::model::SubscriberModel;
use crate::database::model::SubscriberType;
use crate::error::AppError;
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
    #[poise::command(slash_command, guild_only)]
    pub async fn settings(ctx: Context<'_>) -> Result<(), Error> {
        use serenity::futures::StreamExt;

        ctx.defer().await?;

        let guild_id = ctx
            .guild_id()
            .ok_or_else(|| AppError::AssertionError {
                msg: "Cannot get guild id".to_string(),
            })?
            .get();

        let mut settings = ctx
            .data()
            .feed_subscription_service
            .get_server_settings(guild_id)
            .await?;

        let msg_handle = ctx.send(FeedsCog::create_settings_reply(&settings)).await?;

        let msg = msg_handle.message().await?.into_owned();

        let mut collector = ComponentInteractionCollector::new(ctx.serenity_context())
            .message_id(msg.id)
            .timeout(Duration::from_secs(60))
            .stream();

        while let Some(interaction) = collector.next().await {
            match &interaction.data.kind {
                ComponentInteractionDataKind::ChannelSelect { values }
                    if interaction.data.custom_id == "server_settings_channel" =>
                {
                    if let Some(channel_id) = values.first() {
                        settings.channel_id = Some(channel_id.to_string());
                        ctx.data()
                            .feed_subscription_service
                            .update_server_settings(guild_id, settings.clone())
                            .await?;
                    }
                }
                ComponentInteractionDataKind::RoleSelect { values }
                    if interaction.data.custom_id == "server_settings_sub_role" =>
                {
                    if let Some(role_id) = values.first() {
                        settings.subscribe_role_id = Some(role_id.to_string());
                        ctx.data()
                            .feed_subscription_service
                            .update_server_settings(guild_id, settings.clone())
                            .await?;
                    }
                }
                ComponentInteractionDataKind::RoleSelect { values }
                    if interaction.data.custom_id == "server_settings_unsub_role" =>
                {
                    if let Some(role_id) = values.first() {
                        settings.unsubscribe_role_id = Some(role_id.to_string());
                        ctx.data()
                            .feed_subscription_service
                            .update_server_settings(guild_id, settings.clone())
                            .await?;
                    }
                }
                _ => {}
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
        let notification_channel_text = "### Notification Channel\n\n> Select which channel to publish server feed notifications to.";
        let subscribe_permissions_text = "### Subscribe Permissions\n\n> Select who are able to subscribe new feeds to this server.";
        let unsubscribe_permissions_text = "### Unsubscribe Permissions\n\n> Select who are able to unsubscribe existing feeds from this server.";

        // Helper to parsing ids
        let parse_ids = |id: &Option<String>| {
            id.as_ref()
                .and_then(|id| poise::serenity_prelude::RoleId::from_str(id).ok())
                .into_iter()
                .collect::<Vec<_>>()
        };
        let parse_channel_ids = |id: &Option<String>| {
            id.as_ref()
                .and_then(|id| {
                    poise::serenity_prelude::ChannelId::from_str(id)
                        .ok()
                        .map(GenericChannelId::from)
                })
                .into_iter()
                .collect::<Vec<_>>()
        };

        let channel_select = CreateSelectMenu::new(
            "server_settings_channel",
            CreateSelectMenuKind::Channel {
                channel_types: Some(vec![ChannelType::Text, ChannelType::News].into()),
                default_channels: Some(parse_channel_ids(&settings.channel_id).into()),
            },
        )
        .placeholder("Select a notification channel");

        let sub_role_select = CreateSelectMenu::new(
            "server_settings_sub_role",
            CreateSelectMenuKind::Role {
                default_roles: Some(parse_ids(&settings.subscribe_role_id).into()),
            },
        )
        .placeholder("Select role for subscribe permission");

        let unsub_role_select = CreateSelectMenu::new(
            "server_settings_unsub_role",
            CreateSelectMenuKind::Role {
                default_roles: Some(parse_ids(&settings.unsubscribe_role_id).into()),
            },
        )
        .placeholder("Select role for unsubscribe permission");

        let container = CreateComponent::Container(CreateContainer::new(vec![
            CreateContainerComponent::TextDisplay(CreateTextDisplay::new(
                notification_channel_text,
            )),
            CreateContainerComponent::ActionRow(CreateActionRow::SelectMenu(channel_select)),
            CreateContainerComponent::TextDisplay(CreateTextDisplay::new(
                subscribe_permissions_text,
            )),
            CreateContainerComponent::ActionRow(CreateActionRow::SelectMenu(sub_role_select)),
            CreateContainerComponent::TextDisplay(CreateTextDisplay::new(
                unsubscribe_permissions_text,
            )),
            CreateContainerComponent::ActionRow(CreateActionRow::SelectMenu(unsub_role_select)),
        ]));

        vec![container]
    }

    /// Subscribe to a feed
    #[poise::command(slash_command)]
    pub async fn subscribe(
        ctx: Context<'_>,
        #[description = "Link(s) of the feeds. Separate links with commas (,)"] links: String,
        #[description = "Where to send the notifications. Default to your DM"] send_into: Option<
            SendInto,
        >,
    ) -> Result<(), Error> {
        ctx.defer().await?;

        let send_into = send_into.unwrap_or(SendInto::DM);

        let urls_split: Vec<&str> = links.split(',').map(|s| s.trim()).collect();
        if urls_split.len() > 10 {
            Err(BotError::InvalidCommandArgument {
                parameter: "links".to_string(),
                reason: "Too many links provided. Please provide no more than 10 links at a time."
                    .to_string(),
            })?
        };

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

        let urls_split: Vec<&str> = links.split(',').map(|s| s.trim()).collect();
        if urls_split.len() > 10 {
            Err(BotError::InvalidCommandArgument {
                parameter: "links".to_string(),
                reason: "Too many links provided. Please provide no more than 10 links at a time."
                    .to_string(),
            })?
        };

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
        let mut navigation =
            PageNavigationComponent::new(&ctx, Pagination::new(items / per_page + 1, per_page, 1));

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
            let text = CreateTextDisplay::new(format!(
                "### {}

    - **Last version**: {}
    - **Last updated**: <t:{}>
    - **Source**: <{}>
    ﻿",
                sub.feed.name,
                sub.feed_latest.description,
                sub.feed_latest.published.timestamp(),
                sub.feed.url
            ));
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
        let _channel_id = ctx.channel_id();
        let guild_id = ctx
            .guild_id()
            .ok_or_else(|| BotError::InvalidCommandArgument {
                parameter: send_into.name().to_string(),
                reason: "You have to be in a server to do this command with send_into: server"
                    .to_string(),
            })?;

        let ret = match send_into {
            SendInto::Server => guild_id.to_string(),
            SendInto::DM => ctx.author().id.to_string(),
        };
        Ok(ret)
    }
}
