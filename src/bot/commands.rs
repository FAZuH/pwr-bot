use std::collections::HashSet;

use anyhow::Result;
use log::error;
use poise::{ChoiceParameter, CreateReply};
use serenity::all::CreateAttachment;
use sqlx::error::ErrorKind;

use super::bot::Data;
use crate::database::model::LatestResultModel;
use crate::database::model::SubscribersModel;
use crate::database::table::Table;
use crate::source::SourceResult;

type Error = Box<dyn std::error::Error + Send + Sync>;
type Context<'a> = poise::Context<'a, Data, Error>;

#[derive(ChoiceParameter)]
enum SendInto {
    #[name = "dm"]
    DM,
    #[name = "webhook"]
    Webhook,
}

impl SendInto {
    fn as_str(&self) -> &'static str {
        match self {
            Self::DM => "dm",
            Self::Webhook => "webhook",
        }
    }
}

impl std::fmt::Display for SendInto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Subscribe to an anime/manga serise
#[poise::command(slash_command)]
pub async fn subscribe(
    ctx: Context<'_>,
    #[description = "Link(s) of the series. Separate links with commas (,)"] links: String,
    #[description = "Where to send the notifications. Default to DM"] send_into: Option<SendInto>,
) -> Result<(), Error> {
    // 1. Setup
    ctx.defer().await?;
    let user_id = ctx.author().id;
    let data = ctx.data();
    let send_into = send_into.unwrap_or(SendInto::DM);

    let links_split = links.split(",");
    if links_split.clone().count() > 10 {
        ctx.say("Too many links provided. Please provide no more than 10 links at a time.")
            .await?;
        return Ok(());
    };
    for link in links_split {
        // 2. Fetch latest series for the series
        let series_item = match data.sources.get_latest_by_url(link).await {
            Ok(SourceResult::Series(res)) => res,
            Err(_) => {
                ctx.reply(format!("❌ Invalid link: <{link}>")).await?;
                continue;
            }
        };
        let title = series_item.title;
        let latest = series_item.latest;
        let published = series_item.published;

        // 3. latest_result doesn't exist in db => insert it
        // otherwise => get id
        let latest_result = LatestResultModel {
            url: series_item.url, // (1) NOTE: Important for consistent URL
            name: title.clone(),
            latest,
            tags: "series".to_string(),
            published,
            ..Default::default()
        };
        let latest_results_id = match data.db.latest_results_table.insert(&latest_result).await {
            Ok(id) => id,
            Err(_) => {
                match data
                    .db
                    .latest_results_table
                    .select_by_url(&latest_result.url)
                    .await
                {
                    Ok(ok) => ok.id,
                    Err(err) => {
                        ctx.reply(format!("❌ Unexpected error: {err:?}")).await?;
                        continue;
                    }
                }
            }
        };

        // 4. Insert subscriber into db
        let subscriber = SubscribersModel {
            subscriber_id: {
                if send_into.as_str() == "webhook" {
                    data.config.webhook_url.clone()
                } else {
                    user_id.to_string()
                }
            },
            subscriber_type: send_into.as_str().to_string(),
            latest_results_id,
            ..Default::default()
        };
        if let Err(err) = data.db.subscribers_table.insert(&subscriber).await {
            if let Some(db_err) = err.into_database_error() {
                if matches!(db_err.kind(), ErrorKind::UniqueViolation) {
                    ctx.reply(format!("❌ You are already subscribed to {title}"))
                        .await?;
                } else {
                    ctx.reply(format!("⚠️ Unknown error ({db_err:?})")).await?;
                }
            }
        } else {
            ctx.reply(format!(
                "✅ Successfully subscribed to series \"{title}\". Notifications will be sent to {send_into}",
            ))
            .await?;
        }
    }
    Ok(())
}

/// Unsubscribe from an anime/manga serise
#[poise::command(slash_command)]
pub async fn unsubscribe(
    ctx: Context<'_>,
    #[description = "Link(s) of the series. Separate links with commas (,)"]
    #[autocomplete = "autocomplete_subscriptions"]
    links: String,
    #[description = "Where to send the notifications. Default to DM"] send_into: Option<SendInto>,
) -> Result<(), Error> {
    ctx.defer().await?;
    // 1. Setup
    let user_id = ctx.author().id;
    let data = ctx.data();
    let send_into = send_into.unwrap_or(SendInto::DM);

    for link in links.split(',') {
        // 2. Get source from series link
        let source = match data.sources.get_source_by_url(link) {
            Some(source) => source,
            None => {
                ctx.reply(format!("❌ Unsupported link: <{link}>")).await?;
                return Ok(());
            }
        };

        // 3. Get latest series id from db
        //
        // HACK: We do it like this to ensure the link matches the one on the db. See (1)
        let id = match source.get_id_from_url(link) {
            Ok(id) => id,
            Err(err) => {
                ctx.reply(format!("❌ Invalid link ({err:?})")).await?;
                return Ok(());
            }
        };
        let latest_result = match data
            .db
            .latest_results_table
            .select_by_url(&source.get_url_from_id(id))
            .await
        {
            Ok(res) => res,
            Err(err) => {
                ctx.reply(format!("❌ Failed to find series in database ({err:?})"))
                    .await?;
                return Ok(());
            }
        };

        // 4. Delete subscriber based on latest_result_id, subscriber_id, and subscriber_type
        let subscriber = SubscribersModel {
            subscriber_type: send_into.as_str().to_string(),
            subscriber_id: {
                if send_into.as_str() == "webhook" {
                    data.config.webhook_url.clone()
                } else {
                    user_id.to_string()
                }
            },
            latest_results_id: latest_result.id,
            ..Default::default()
        };
        if data
            .db
            .subscribers_table
            .delete_by_model(subscriber)
            .await?
        {
            ctx.reply(format!(
                "✅ Successfully unsubscribed to series {} on {}",
                latest_result.name,
                source.get_url().name
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
pub async fn subscriptions(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer().await?;
    let user_id = ctx.author().id.to_string();
    let data = ctx.data();

    let subscriptions = data
        .db
        .subscribers_table
        .select_all_by_subscriber_id(&user_id)
        .await?;

    if subscriptions.is_empty() {
        ctx.reply("You are not subscribed to any series.").await?;
        return Ok(());
    }

    let mut message = "You are subscribed to the following series:
"
    .to_string();
    for subscription in subscriptions {
        let LatestResultModel { name, url, .. } = data
            .db
            .latest_results_table
            .select(&subscription.latest_results_id)
            .await?;

        message.push_str(&format!("- [{name}](<{url}>)\n",));
    }

    ctx.reply(message).await?;

    Ok(())
}

/// Help command to show all available commands
#[poise::command(slash_command)]
pub async fn help(
    ctx: Context<'_>,
    #[description = "Specific command to show help about"]
    #[autocomplete = "poise::builtins::autocomplete_command"]
    command: Option<String>,
) -> Result<(), Error> {
    ctx.defer().await?;
    poise::builtins::help(
        ctx,
        command.as_deref(),
        poise::builtins::HelpConfiguration {
            ..Default::default()
        },
    )
    .await?;
    Ok(())
}

#[poise::command(prefix_command, owners_only, hide_in_help)]
pub async fn register(ctx: Context<'_>) -> Result<(), Error> {
    poise::builtins::register_application_commands_buttons(ctx).await?;
    Ok(())
}

#[poise::command(slash_command, owners_only, hide_in_help)]
pub async fn dump_db(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer().await?;
    let data = ctx.data();

    let subscribers = data.db.subscribers_table.select_all().await?;
    let latest_results = data.db.latest_results_table.select_all().await?;

    let subscribers_json = serde_json::to_string_pretty(&subscribers)?;
    let latest_results_json = serde_json::to_string_pretty(&latest_results)?;

    let reply = CreateReply::default()
        .content("Database dump:")
        .attachment(CreateAttachment::bytes(
            subscribers_json.as_bytes(),
            "subscribers.json",
        ))
        .attachment(CreateAttachment::bytes(
            latest_results_json.as_bytes(),
            "latest_results.json",
        ));

    if let Err(e) = ctx.send(reply).await {
        let _ = ctx.reply(format!("Failed to send: {}", e)).await;
    }
    Ok(())
}

async fn autocomplete_subscriptions(ctx: Context<'_>, partial: &str) -> Vec<String> {
    // Early exit if partial is empty
    if partial.trim().is_empty() {
        return Vec::new();
    }

    let user_id = ctx.author().id.to_string();

    // Get subscriptions
    let subscriptions = match ctx
        .data()
        .db
        .subscribers_table
        .select_all_by_subscriber_id(&user_id)
        .await
    {
        Ok(subs) => subs,
        Err(e) => {
            error!("Failed to fetch subscriptions for user {}: {}", user_id, e);
            return Vec::new();
        }
    };

    if subscriptions.is_empty() {
        return Vec::new();
    }

    // Extract unique latest_results_ids
    let unique_ids: HashSet<u32> = subscriptions
        .into_iter()
        .map(|sub| sub.latest_results_id)
        .collect();

    // Fetch all results in parallel and filter
    let futures = unique_ids.into_iter().map(|id| {
        let db = &ctx.data().db;
        async move { db.latest_results_table.select(&id).await.ok() }
    });
    let results = futures::future::join_all(futures).await;

    let partial_lower = partial.to_lowercase();
    let mut matching_urls: Vec<String> = results
        .into_iter()
        .flatten()
        .filter(|res| res.url.to_lowercase().contains(&partial_lower))
        .map(|res| res.url)
        .collect();

    matching_urls.sort();
    matching_urls.truncate(25); // Discord autocomplete limit
    matching_urls
}
