use std::collections::HashSet;
use std::fmt::Display;
use std::time::Duration;
use std::time::Instant;

use anyhow::Result;
use log::error;
use poise::ChoiceParameter;
use poise::CreateReply;
use poise::ReplyHandle;
use poise::serenity_prelude::AutocompleteChoice;
use poise::serenity_prelude::CreateAttachment;
use poise::serenity_prelude::CreateAutocompleteResponse;
use serenity::all::CreateComponent;
use serenity::all::CreateContainer;
use serenity::all::CreateContainerComponent;
use serenity::all::CreateSection;
use serenity::all::CreateSectionAccessory;
use serenity::all::CreateSectionComponent;
use serenity::all::CreateTextDisplay;
use serenity::all::CreateThumbnail;
use serenity::all::CreateUnfurledMediaItem;
use serenity::all::MessageFlags;

use crate::bot::Data;
use crate::bot::components::PageNavigationComponent;
use crate::bot::components::Pagination;
use crate::bot::error::BotError;
use crate::database::model::SubscriberModel;
use crate::database::model::SubscriberType;
use crate::database::table::Table;
use crate::service::series_feed_subscription_service::SubscribeResult;
use crate::service::series_feed_subscription_service::SubscriberTarget;
use crate::service::series_feed_subscription_service::UnsubscribeResult;

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Context<'a> = poise::Context<'a, Data, Error>;

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

pub struct Commands;

impl Commands {
    /// Subscribe to an anime/manga series
    #[poise::command(slash_command)]
    pub async fn subscribe(
        ctx: Context<'_>,
        #[description = "Link(s) of the series. Separate links with commas (,)"] links: String,
        #[description = "Where to send the notifications. Default to DM"] send_into: Option<
            SendInto,
        >,
    ) -> Result<(), Error> {
        ctx.defer().await?;

        let send_into = send_into.unwrap_or(SendInto::DM);

        let subscriber_type = SubscriberType::from(&send_into);
        let target_id = Commands::get_target_id(ctx, &send_into)?;

        let urls_split: Vec<&str> = links.split(',').map(|s| s.trim()).collect();
        if urls_split.len() > 10 {
            Err(BotError::InvalidCommandArgument {
                parameter: "links".to_string(),
                reason: "Too many links provided. Please provide no more than 10 links at a time."
                    .to_string(),
            })?
        };

        let mut states: Vec<String> =
            vec!["<a:loading:466940188849995807> ﻿ Processing...".to_string(); urls_split.len()];

        let interval = Duration::from_secs(2);
        let mut last_send = Instant::now();

        let mut reply: Option<ReplyHandle<'_>> = None;

        // NOTE: Can be done concurrently
        for (i, url) in urls_split.iter().enumerate() {
            let target = SubscriberTarget {
                subscriber_type,
                target_id: target_id.clone(),
            };

            let sub_result = ctx
                .data()
                .series_feed_subscription_service
                .subscribe(url, target)
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

            if last_send.elapsed() > interval {
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

    /// Unsubscribe from an anime/manga series
    #[poise::command(slash_command)]
    pub async fn unsubscribe(
        ctx: Context<'_>,
        #[description = "Link(s) of the series. Separate links with commas (,)"]
        #[autocomplete = "Self::autocomplete_subscriptions"]
        links: String,
        #[description = "Where notifications were being sent. Default to DM"] send_into: Option<
            SendInto,
        >,
    ) -> Result<(), Error> {
        ctx.defer().await?;

        let send_into = send_into.unwrap_or(SendInto::DM);

        let subscriber_type = SubscriberType::from(&send_into);
        let target_id = Commands::get_target_id(ctx, &send_into)?;

        let urls_split: Vec<&str> = links.split(',').map(|s| s.trim()).collect();
        if urls_split.len() > 10 {
            Err(BotError::InvalidCommandArgument {
                parameter: "links".to_string(),
                reason: "Too many links provided. Please provide no more than 10 links at a time."
                    .to_string(),
            })?
        };

        let mut states: Vec<String> =
            vec!["<a:loading:466940188849995807> ﻿ Processing...".to_string(); urls_split.len()];

        let interval = Duration::from_secs(2);
        let mut last_send = Instant::now();

        let mut reply: Option<ReplyHandle<'_>> = None;

        // NOTE: Can be done concurrently
        for (i, url) in urls_split.iter().enumerate() {
            let target = SubscriberTarget {
                subscriber_type,
                target_id: target_id.clone(),
            };

            let unsub_result = ctx
                .data()
                .series_feed_subscription_service
                .unsubscribe(url, target)
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

            if last_send.elapsed() > interval {
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

    /// List all your subscriptions
    #[poise::command(slash_command)]
    pub async fn subscriptions(
        ctx: Context<'_>,
        #[description = "Where the notifications are being sent. Default to DM"] sent_into: Option<
            SendInto,
        >,
    ) -> Result<(), Error> {
        ctx.defer().await?;
        let sent_into = sent_into.unwrap_or(SendInto::DM);

        // Create target
        let target_id = Commands::get_target_id(ctx, &sent_into)?;
        let subscriber_type = SubscriberType::from(&sent_into);
        let target = SubscriberTarget {
            subscriber_type,
            target_id,
        };

        // Get subscriber
        let subscriber = ctx
            .data()
            .series_feed_subscription_service
            .get_or_create_subscriber(&target)
            .await?;

        // Get subscriber's subscription count
        let pages = ctx
            .data()
            .series_feed_subscription_service
            .get_subscription_count(subscriber.id)
            .await?;
        if pages == 0 {
            ctx.reply("You have no subscriptions.").await?;
            return Ok(());
        }

        // Create navigation component
        let mut navigation = PageNavigationComponent::new(&ctx, Pagination::new(pages, 10, 1));

        // Run feedback loop until timeout
        let reply = ctx
            .send(
                CreateReply::new()
                    .components(Commands::create_page(&ctx, &target, &navigation).await?),
            )
            .await?;

        while navigation.listen(Duration::from_secs(60)).await {
            reply
                .edit(
                    ctx,
                    CreateReply::new()
                        .components(Commands::create_page(&ctx, &target, &navigation).await?),
                )
                .await?;
        }

        Ok(())
    }

    async fn create_page<'a>(
        ctx: &Context<'_>,
        target: &SubscriberTarget,
        navigation: &'a PageNavigationComponent<'_>,
    ) -> anyhow::Result<Vec<CreateComponent<'a>>> {
        let mut container_components = vec![];
        let subscriptions = ctx
            .data()
            .series_feed_subscription_service
            .list_paginated_subscriptions(
                target,
                navigation.pagination.current_page,
                navigation.pagination.pages,
            )
            .await?;
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
        let buttons = navigation.create_buttons();

        Ok(vec![container, buttons])
    }

    #[poise::command(prefix_command, owners_only, hide_in_help)]
    pub async fn register(ctx: Context<'_>) -> Result<(), Error> {
        poise::builtins::register_application_commands_buttons(ctx).await?;
        Ok(())
    }

    #[poise::command(prefix_command, owners_only, hide_in_help)]
    pub async fn dump_db(ctx: Context<'_>) -> Result<(), Error> {
        ctx.defer().await?;
        let data = ctx.data();

        let feeds = data.db.feed_table.select_all().await?;
        let versions = data.db.feed_item_table.select_all().await?;
        let subscribers = data.db.subscriber_table.select_all().await?;
        let subscriptions = data.db.feed_subscription_table.select_all().await?;

        let reply = CreateReply::default()
            .content("Database dump:")
            .attachment(CreateAttachment::bytes(
                serde_json::to_string_pretty(&feeds)?,
                "feeds.json",
            ))
            .attachment(CreateAttachment::bytes(
                serde_json::to_string_pretty(&versions)?,
                "feed_versions.json",
            ))
            .attachment(CreateAttachment::bytes(
                serde_json::to_string_pretty(&subscribers)?,
                "subscribers.json",
            ))
            .attachment(CreateAttachment::bytes(
                serde_json::to_string_pretty(&subscriptions)?,
                "subscriptions.json",
            ));

        ctx.send(reply).await?;
        Ok(())
    }

    async fn autocomplete_subscriptions<'a>(
        ctx: Context<'_>,
        partial: &str,
    ) -> CreateAutocompleteResponse<'a> {
        if partial.trim().is_empty() {
            return CreateAutocompleteResponse::new().set_choices(vec![]);
        }

        let data = ctx.data();
        let user_id = ctx.author().id.to_string();

        // Find subscriber
        let subscriber = match data
            .db
            .subscriber_table
            .select_by_type_and_target(&SubscriberType::Dm, &user_id)
            .await
        {
            Ok(sub) => sub,
            Err(e) => {
                error!("Failed to fetch subscriber for user {}: {}", user_id, e);
                return CreateAutocompleteResponse::new().set_choices(vec![]);
            }
        };

        // Get subscriptions
        let subscriptions = match data
            .db
            .feed_subscription_table
            .select_all_by_subscriber_id(subscriber.id)
            .await
        {
            Ok(subs) => subs,
            Err(e) => {
                error!("Failed to fetch subscriptions: {}", e);
                return CreateAutocompleteResponse::new().set_choices(vec![]);
            }
        };

        if subscriptions.is_empty() {
            return CreateAutocompleteResponse::new().set_choices(vec![]);
        }

        // Fetch feeds in parallel
        // TODO: Do this in backend instead
        let feed_ids: HashSet<i32> = subscriptions.into_iter().map(|s| s.feed_id).collect();
        let futures = feed_ids.into_iter().map(|id| {
            let db = &data.db;
            async move { db.feed_table.select(&id).await.ok() }
        });
        let results = futures::future::join_all(futures).await;

        let partial_lower = partial.to_lowercase();
        // 1. Collect into a Vec of Feeds
        let mut matching_urls = results
            .into_iter()
            .flatten()
            .filter(|feed| feed.url.to_lowercase().contains(&partial_lower))
            .collect::<Vec<_>>();

        // 2. Sort the Feeds
        matching_urls.sort_by_key(|feed| feed.name.to_lowercase());

        // 3. Map the Feeds into AutocompleteChoices
        let mut choices = matching_urls
            .into_iter()
            .map(|feed| AutocompleteChoice::new(feed.name, feed.url))
            .collect::<Vec<_>>();

        choices.truncate(25); // Discord autocomplete limit
        CreateAutocompleteResponse::new().set_choices(choices)
    }

    fn get_target_id(ctx: Context<'_>, send_into: &SendInto) -> Result<String, BotError> {
        let channel_id = ctx.channel_id();
        let guild_id = ctx
            .guild_id()
            .ok_or_else(|| BotError::InvalidCommandArgument {
                parameter: send_into.name().to_string(),
                reason: "You have to be in a server to do this command with send_into: server"
                    .to_string(),
            })?;

        let ret = match send_into {
            SendInto::Server => SubscriberModel::format_guild_target_id(guild_id, channel_id),
            SendInto::DM => ctx.author().id.to_string(),
        };
        Ok(ret)
    }
}
