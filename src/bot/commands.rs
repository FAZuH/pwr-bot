use anyhow::Result;
use sqlx::error::ErrorKind;
use chrono::{DateTime, Utc};
use poise::{ChoiceParameter, CreateReply};
use serenity::all::CreateAttachment;

use super::bot::Data;
use crate::database::model::latest_updates_model::LatestUpdatesModel;
use crate::database::model::subscribers_model::SubscribersModel;
use crate::database::table::table::Table;

type Error = Box<dyn std::error::Error + Send + Sync>;
type Context<'a> = poise::Context<'a, Data, Error>;

#[derive(ChoiceParameter)]
enum SeriesType {
    #[name = "Anime"]
    Anime,
    #[name = "Manga"]
    Manga,
}

impl SeriesType {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Anime => "anime",
            Self::Manga => "manga",
        }
    }
}

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

/// Subscribe to an anime/manga serise
#[poise::command(slash_command)]
pub async fn subscribe(
    ctx: Context<'_>,
    #[description = "Type of series"] series_type: SeriesType,
    #[description = "ID/link of the series(s)"] series: String,
    #[description = "Where to send the notifications. Default to DM"] send_into: Option<SendInto>,
) -> Result<(), Error> {

    // 1. Setup
    let user_id = ctx.author().id.to_string();
    let data = ctx.data();
    let send_into = send_into.unwrap_or(SendInto::DM);

    // 2. Fetch latest update for the series
    let series_id: String;
    let series_title: String;
    let series_latest: String;
    let series_published: DateTime<Utc>;
    match series_type {
        SeriesType::Manga => {
            series_id = data.manga_source.get_id_from_url(&series).unwrap_or(series);
            match data.manga_source.get_latest(&series_id).await {
                Ok(res) => {
                    // get_title returns Ok(Manga) <=> get_latest returns Ok(Manga)
                    series_title = data.manga_source.get_title(&series_id).await?;
                    series_latest = res.chapter;
                    series_published = res.published;
                }
                Err(err) => {
                    ctx.reply(format!(
                        "❌ Manga with series id \"{series_id}\" not found on https://{}: ({})",
                        data.manga_source.base.api_domain, err
                    ))
                    .await?;
                    return Ok(());
                }
            }
        }
        SeriesType::Anime => {
            series_id = data.anime_source.get_id_from_url(&series).unwrap_or(series);
            match data.anime_source.get_latest(&series_id).await {
                Ok(res) => {
                    // get_title returns Ok(Anime) <=> get_latest returns Ok(Anime)
                    series_title = res.title;
                    series_latest = res.episode;
                    series_published = res.published;
                }
                Err(err) => {
                    ctx.reply(format!(
                        "❌ Anime with series id \"{series_id}\" not found on https://{} ({})",
                        data.anime_source.base.api_domain, err
                    ))
                    .await?;
                    return Ok(());
                }
            }
        }
    }

    // 3. latest_update doesn't exist in db => insert it
    // otherwise => get id
    let latest_update = LatestUpdatesModel {
        r#type: series_type.as_str().to_string(),
        series_id: series_id.clone(),
        series_latest,
        series_title: series_title.clone(),
        series_published,
        ..Default::default()
    };
    let latest_update_id = match data.db.latest_updates_table.insert(&latest_update).await {
        Ok(id) => id,
        Err(_) => {
            data.db
                .latest_updates_table
                .select_by_model(&latest_update)
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
        latest_update_id,
        ..Default::default()
    };
    if let Err(err) = data.db.subscribers_table.insert(&subscriber).await {
        if let Some(db_err) = err.into_database_error() {
            if matches!(db_err.kind(), ErrorKind::UniqueViolation) {
                ctx.reply(format!("You are already subscribed to {series_title}")).await?;
            } else {
                ctx.reply(format!("Unknown error: {db_err}")).await?;
            }
        }
    } else {
        ctx.reply(format!(
            "✅ Successfully subscribed to \"{}\" series \"{}\". Notifications will be sent to {}",
            series_type.as_str(),
            series_title,
            send_into.as_str()
        ))
        .await?;
    }

    Ok(())
}

/// Unsubscribe from an anime/manga serise
#[poise::command(slash_command)]
pub async fn unsubscribe(
    ctx: Context<'_>,
    #[description = "Type of series"] series_type: SeriesType,
    #[description = "ID/link of the series(s)"] series: String,
    #[description = "Where to send the notifications. Default to DM"] send_into: Option<SendInto>,
) -> Result<(), Error> {

    // 1. Setup
    let user_id = ctx.author().id.to_string();
    let data = ctx.data();
    let send_into = send_into.unwrap_or(SendInto::DM);

    // 2. Get series_id from series link if needed
    let series_id = match series_type {
        SeriesType::Manga => data.manga_source.get_id_from_url(&series).unwrap_or(series),
        SeriesType::Anime => data.anime_source.get_id_from_url(&series).unwrap_or(series),
    };

    // 3. Get latest update id from db
    let latest_update = LatestUpdatesModel {
        r#type: series_type.as_str().to_string(),
        series_id: series_id.clone(),
        ..Default::default()
    };
    let latest_update_id = if let Ok(res) = data
        .db
        .latest_updates_table
        .select_by_model(&latest_update)
        .await
    {
        res.id
    } else {
        ctx.reply(format!(
            "❌ type \"{}\" and series_id \"{}\" not found in the database",
            latest_update.r#type, latest_update.series_id
        ))
        .await?;
        return Ok(());
    };

    // 4. Delete subscriber based on latest_update_id, subscriber_id, and subscriber_type
    let subscriber = SubscribersModel {
        subscriber_type: send_into.as_str().to_string(),
        subscriber_id: {
            if send_into.as_str() == "webhook" { data.config.webhook_url.clone() } else { user_id
            }
        },
        latest_update_id,
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
            series_type.as_str(),
            latest_update.series_title
        ))
        .await?;
    } else {
        ctx.reply("❌ You are not subscribed to this series").await?;
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
".to_string();
    for subscription in subscriptions {
        let latest_update = data
            .db
            .latest_updates_table
            .select(&subscription.latest_update_id)
            .await?;

        message.push_str(&format!(
            "- {} `{}` ({})\n",
            latest_update.r#type, latest_update.series_title, latest_update.series_id
        ));
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
    let latest_updates = data.db.latest_updates_table.select_all().await?;

    let subscribers_json = serde_json::to_string_pretty(&subscribers)?;
    let latest_updates_json = serde_json::to_string_pretty(&latest_updates)?;

    let reply = CreateReply::default()
        .content("Database dump:")
        .attachment(CreateAttachment::bytes(subscribers_json.as_bytes(), "subscribers.json"))
        .attachment(CreateAttachment::bytes(latest_updates_json.as_bytes(), "latest_updates.json"));

    if let Err(e) = ctx.send(reply).await {
        let _ = ctx.reply(format!("Failed to send: {}", e)).await;
    }
    Ok(())
}

// #[poise::command(slash_command, owners_only, hide_in_help)]
// pub async fn add_owner(ctx: Context<'_>) -> Result<(), Error> {
//     let user_id = ctx.author().id.to_string();
//     let data = ctx.data();
//     ctx.reply(format!("Successfully added {} as an owner.", user_id)).await?;
//     Ok(())
// }
