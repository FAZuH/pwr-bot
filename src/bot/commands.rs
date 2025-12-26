use std::collections::HashSet;
use std::fmt::Display;

use anyhow::Result;
use log::error;
use poise::ChoiceParameter;
use poise::CreateReply;
use poise::serenity_prelude::AutocompleteChoice;
use poise::serenity_prelude::CreateAttachment;
use poise::serenity_prelude::CreateAutocompleteResponse;
use sqlx::error::ErrorKind;

use crate::bot::Data;
use crate::bot::error::BotError;
use crate::database::model::FeedItemModel;
use crate::database::model::FeedModel;
use crate::database::model::FeedSubscriptionModel;
use crate::database::model::SubscriberModel;
use crate::database::model::SubscriberType;
use crate::database::table::Table;
use crate::feed::error::SeriesError;

type Error = Box<dyn std::error::Error + Send + Sync>;
type Context<'a> = poise::Context<'a, Data, Error>;

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

pub struct Commands {}

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

        let data = ctx.data();
        let send_into = send_into.unwrap_or(SendInto::DM);

        let target_id = Commands::get_target_id(ctx, &send_into)?;
        let subscriber_type = SubscriberType::from(&send_into);

        let links_split: Vec<&str> = links.split(',').map(|s| s.trim()).collect();
        if links_split.len() > 10 {
            ctx.say("Too many links provided. Please provide no more than 10 links at a time.")
                .await?;
            return Ok(());
        }

        for link in links_split {
            let source =
                data.sources
                    .get_feed_by_url(link)
                    .ok_or_else(|| SeriesError::UnsupportedUrl {
                        url: link.to_string(),
                    })?;

            let id = source.get_id_from_url(link)?;
            let series_latest = source.get_latest(id).await?;

            let feed = match data.db.feed_table.select_by_url(&series_latest.url).await {
                Ok(feed) => feed,
                Err(_) => {
                    // Feed doesn't exist, create it
                    let series_info = source.get_info(id).await?;

                    let mut feed = FeedModel {
                        name: series_info.title.clone(),
                        description: series_info.description,
                        url: series_info.url.clone(),
                        cover_url: series_info.cover_url.unwrap_or("".to_string()),
                        tags: "series".to_string(),
                        ..Default::default()
                    };
                    feed.id = data.db.feed_table.insert(&feed).await?;

                    // Create initial version
                    let version = FeedItemModel {
                        feed_id: feed.id,
                        description: series_latest.latest.clone(),
                        published: series_latest.published,
                        ..Default::default()
                    };
                    data.db.feed_item_table.insert(&version).await?;

                    feed
                }
            };

            // Get or create subscriber
            let subscriber = data
                .db
                .subscriber_table
                .select_by_type_and_target(&subscriber_type, &target_id)
                .await;
            let subscriber_id = match subscriber {
                Ok(existing) => existing.id,
                Err(_) => {
                    // Subscriber doesn't exist, create it
                    let subscriber = SubscriberModel {
                        r#type: subscriber_type,
                        target_id: target_id.clone(),
                        ..Default::default()
                    };
                    data.db.subscriber_table.insert(&subscriber).await?
                }
            };

            // Create subscription
            let subscription = FeedSubscriptionModel {
                feed_id: feed.id,
                subscriber_id,
                ..Default::default()
            };
            match data.db.feed_subscription_table.insert(&subscription).await {
                Ok(_) => {
                    ctx.reply(format!(
                        "✅ Successfully subscribed to \"{}\". Notifications will be sent to {}",
                        feed.name, send_into
                    ))
                    .await?;
                }
                Err(err) => {
                    let err_msg = err.to_string();
                    if let Some(db_err) = err.into_database_error() {
                        if matches!(db_err.kind(), ErrorKind::UniqueViolation) {
                            ctx.reply(format!("❌ Already subscribed to {}", feed.name))
                                .await?;
                        } else {
                            ctx.reply(format!("⚠️ Unknown error: {db_err:?}")).await?;
                        }
                    } else {
                        ctx.reply(format!("⚠️ Error: {err_msg:?}")).await?;
                    }
                }
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

        let data = ctx.data();
        let send_into = send_into.unwrap_or(SendInto::DM);

        let target_id = Commands::get_target_id(ctx, &send_into)?;
        let subscriber_type = SubscriberType::from(&send_into);

        for link in links.split(',').map(|s| s.trim()) {
            // Get source and normalize URL
            let source = match data.sources.get_feed_by_url(link) {
                Some(source) => source,
                None => {
                    ctx.reply(format!("❌ Unsupported link: <{link}>")).await?;
                    continue;
                }
            };

            let id = match source.get_id_from_url(link) {
                Ok(id) => id,
                Err(err) => {
                    ctx.reply(format!("❌ Invalid link: {err:?}")).await?;
                    continue;
                }
            };

            let normalized_url = source.get_url_from_id(id);

            // Find the feed
            let feed = match data.db.feed_table.select_by_url(&normalized_url).await {
                Ok(feed) => feed,
                Err(_) => {
                    ctx.reply("❌ Series not found in database").await?;
                    continue;
                }
            };

            // Find the subscriber
            let subscriber = match data
                .db
                .subscriber_table
                .select_by_type_and_target(&subscriber_type, &target_id)
                .await
            {
                Ok(sub) => sub,
                Err(_) => {
                    ctx.reply("❌ You are not subscribed to this series")
                        .await?;
                    continue;
                }
            };

            // Delete the subscription
            if data
                .db
                .feed_subscription_table
                .delete_subscription(feed.id, subscriber.id)
                .await?
            {
                ctx.reply(format!(
                    "✅ Successfully unsubscribed from \"{}\"",
                    feed.name
                ))
                .await?;
            } else {
                ctx.reply("❌ You are not subscribed to this series")
                    .await?;
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

        let data = ctx.data();
        let sent_into = sent_into.unwrap_or(SendInto::DM);

        let target_id = Commands::get_target_id(ctx, &sent_into)?;
        let subscriber_type = SubscriberType::from(&sent_into);

        // Find subscriber
        let subscriber = match data
            .db
            .subscriber_table
            .select_by_type_and_target(&subscriber_type, &target_id)
            .await
        {
            Ok(sub) => sub,
            Err(_) => {
                ctx.reply("You have no subscriptions.").await?;
                return Ok(());
            }
        };

        // Get all subscriptions
        let subscriptions = data
            .db
            .feed_subscription_table
            .select_all_by_subscriber_id(subscriber.id)
            .await?;

        if subscriptions.is_empty() {
            ctx.reply("You have no subscriptions.").await?;
            return Ok(());
        }

        let mut message = "Your subscriptions:\n".to_string();
        for subscription in subscriptions {
            let feed = data.db.feed_table.select(&subscription.feed_id).await?;

            // Get latest version
            let latest = match data
                .db
                .feed_item_table
                .select_latest_by_feed_id(feed.id)
                .await
            {
                Ok(ver) => ver.description,
                Err(_) => "Unknown".to_string(),
            };

            message.push_str(&format!(
                "- [{}](<{}>) - Latest: {}\n",
                feed.name, feed.url, latest
            ));
        }

        ctx.reply(message).await?;
        Ok(())
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
