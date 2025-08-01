use anyhow::Result;
use poise::{ChoiceParameter, CreateReply};
use serenity::all::CreateAttachment;
use sqlx::error::ErrorKind;

use super::bot::Data;
use crate::database::model::latest_results_model::LatestResultModel;
use crate::database::model::subscribers_model::SubscribersModel;
use crate::database::table::Table;
use crate::source::model::SourceResult;

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
    #[description = "Link of the series"] link: String,
    #[description = "Where to send the notifications. Default to DM"] send_into: Option<SendInto>,
) -> Result<(), Error> {
    // 1. Setup
    let user_id = ctx.author().id.to_string();
    let data = ctx.data();
    let send_into = send_into.unwrap_or(SendInto::DM);

    // 2. Fetch latest series for the series
    let SourceResult::Series(series_item) = data.sources.get_latest_by_url(&link).await?;
    // let series_item = match data.sources.get_latest_by_url(&link).await? {
    //     SourceResult::Series(series_item) => series_item,
    // _ => {
    //     ctx.reply(format!("❌ Invalid URL: {}", series_id)).await?;
    //     return Ok(());
    // }
    // };
    let title = series_item.title;
    let latest = series_item.latest;
    let published = series_item.published;

    // 3. latest_result doesn't exist in db => insert it
    // otherwise => get id
    let latest_result = LatestResultModel {
        url: series_item.url, // (1) NOTE: Important for consistent URL
        latest,
        name: title.clone(),
        published,
        ..Default::default()
    };
    let latest_results_id = match data.db.latest_results_table.insert(&latest_result).await {
        Ok(id) => id,
        Err(_) => {
            data.db
                .latest_results_table
                .select_by_url(&latest_result.url)
                .await?
                .id
        }
    };

    // 4. Insert subscriber into db
    let subscriber = SubscribersModel {
        subscriber_id: {
            if send_into.as_str() == "webhook" {
                data.config.webhook_url.clone()
            } else {
                user_id
            }
        },
        subscriber_type: send_into.as_str().to_string(),
        latest_results_id,
        ..Default::default()
    };
    if let Err(err) = data.db.subscribers_table.insert(&subscriber).await {
        if let Some(db_err) = err.into_database_error() {
            if matches!(db_err.kind(), ErrorKind::UniqueViolation) {
                ctx.reply(format!("You are already subscribed to {title}"))
                    .await?;
            } else {
                ctx.reply(format!("Unknown error: {db_err}")).await?;
            }
        }
    } else {
        ctx.reply(format!(
            "✅ Successfully subscribed to series \"{title}\". Notifications will be sent to {send_into}",
        ))
        .await?;
    }

    Ok(())
}

/// Unsubscribe from an anime/manga serise
#[poise::command(slash_command)]
pub async fn unsubscribe(
    ctx: Context<'_>,
    #[description = "Link of the series"] link: String,
    #[description = "Where to send the notifications. Default to DM"] send_into: Option<SendInto>,
) -> Result<(), Error> {
    // 1. Setup
    let user_id = ctx.author().id.to_string();
    let data = ctx.data();
    let send_into = send_into.unwrap_or(SendInto::DM);

    // 2. Get source from series link
    let source = match data.sources.get_source_by_url(&link) {
        Some(source) => source,
        None => {
            ctx.reply(format!("❌ Unsupported URL: {link}")).await?;
            return Ok(());
        }
    };

    // 3. Get latest series id from db
    //
    // HACK: We do it like this to make sure the link matches the one on the db. See (1)
    let id = source.get_id_from_url(&link)?; // Assuming link is already validated on step 2
    let latest_result = match data
        .db
        .latest_results_table
        .select_by_url(&source.get_url_from_id(id))
        .await
    {
        Ok(res) => res,
        Err(err) => {
            ctx.reply(format!("❌ Failed to find series in database: {err:?}"))
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
                user_id
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
            "✅ Successfully unsubscribed from {} series {}",
            source.get_url().name,
            latest_result.name
        ))
        .await?;
    } else {
        ctx.reply("❌ You are not subscribed to this series")
            .await?;
    }

    Ok(())
}

/// List all your subscriptions
#[poise::command(slash_command)]
pub async fn subscriptions(ctx: Context<'_>) -> Result<(), Error> {
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

        message.push_str(&format!("- {name} ({url})\n",));
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
