use anyhow::Result;
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

#[poise::command(slash_command)]
pub async fn subscribe(
    ctx: Context<'_>,
    #[description = "Type of series"] series_type: SeriesType,
    #[description = "ID of the series"] series_id: String,
    #[description = "Where to send the notifications"] send_into: SendInto,
) -> Result<(), Error> {
    let user_id = ctx.author().id.to_string();
    let data = ctx.data();

    let series_title: String;
    let series_latest: String;
    let series_published: DateTime<Utc>;
    match series_type {
        SeriesType::Manga => {
            if let Ok(res) = data.manga_source.get_latest(&series_id).await {
                // get_title returns Ok(Manga) <=> get_latest returns Ok(Manga)
                series_title = data.manga_source.get_title(&series_id).await?;
                series_latest = res.chapter;
                series_published = res.published;
            } else {
                ctx.say(format!(
                    "❌ Manga with series id \"{}\" not found on MangaDex",
                    series_id
                ))
                .await?;
                return Ok(());
            }
        }
        SeriesType::Anime => {
            if let Ok(res) = data.anime_source.get_latest(&series_id).await {
                series_title = res.title;
                series_latest = res.episode;
                series_published = res.published;
            } else {
                ctx.say(format!(
                    "❌ Anime with series id \"{}\" not found on AniList",
                    series_id
                ))
                .await?;
                return Ok(());
            }
        }
    }

    // latest_update doesn't exist in db => insert it
    // otherwise => get id
    let latest_update = LatestUpdatesModel {
        id: 0,
        r#type: series_type.as_str().to_string(),
        series_id: series_id.clone(),
        series_latest: series_latest,
        series_title: series_title.clone(),
        series_published: series_published,
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

    let subscriber = SubscribersModel {
        id: 0,
        subscriber_id: {
            if send_into.as_str() == "webhook" {
                data.config.webhook_url.clone()
            } else {
                user_id
            }
        },
        subscriber_type: send_into.as_str().to_string(),
        latest_update_id: latest_update_id,
    };

    // TODO: More robust handling needed
    if let Err(err) = data.db.subscribers_table.insert(&subscriber).await {
        if err.to_string().contains("code: 2067") {
            ctx.say(format!(
                "You are already subscribed to this {} series",
                series_type.as_str()
            )).await?;
        } else {
            ctx.say(format!(
                "An error occurred: {}",
                err
            )).await?;
        }
        return Ok(());
    };

    ctx.say(format!(
        "✅ Successfully subscribed to \"{}\" series \"{}",
        series_type.as_str(),
        series_title
    ))
    .await?;

    Ok(())
}

#[poise::command(slash_command)]
pub async fn unsubscribe(
    ctx: Context<'_>,
    #[description = "Type of series"] series_type: SeriesType,
    #[description = "ID of the series"] series_id: String,
    #[description = "Where the notifications were sent"] send_into: SendInto,
) -> Result<(), Error> {
    let user_id = ctx.author().id.to_string();
    let data = ctx.data();

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
        ctx.say(format!(
            "❌ type \"{}\" and series_id \"{}\" not found in the. database",
            latest_update.r#type, latest_update.series_id
        ))
        .await?;
        return Ok(());
    };

    let subscriber_id = {
        if send_into.as_str() == "webhook" {
            data.config.webhook_url.clone()
        } else {
            user_id
        }
    };
    let subscriber = SubscribersModel {
        id: 0,
        subscriber_type: send_into.as_str().to_string(),
        subscriber_id: subscriber_id.clone(),
        latest_update_id: latest_update_id,
    };

    if data
        .db
        .subscribers_table
        .delete_by_model(subscriber)
        .await?
    {
        ctx.say(format!(
            "✅ Successfully unsubscribed from {} series `{}`",
            series_type.as_str(),
            series_id
        ))
        .await?;
    } else {
        ctx.say("❌ You are not subscribed to this series").await?;
    }

    Ok(())
}

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
        ctx.say("You are not subscribed to any series.").await?;
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

    ctx.say(message).await?;

    Ok(())
}

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
        let _ = ctx.say(format!("Failed to send: {}", e)).await;
    }
    Ok(())
}

// #[poise::command(slash_command, owners_only, hide_in_help)]
// pub async fn add_owner(ctx: Context<'_>) -> Result<(), Error> {
//     let user_id = ctx.author().id.to_string();
//     let data = ctx.data();
//     ctx.say(format!("Successfully added {} as an owner.", user_id)).await?;
//     Ok(())
// }
