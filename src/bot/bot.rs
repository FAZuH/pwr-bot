use anyhow;
use chrono::{DateTime, Utc};
use poise::ChoiceParameter;
use std::sync::Arc;

use crate::database::model::latest_updates_model::LatestUpdatesModel;
use crate::database::model::subscribers_model::SubscribersModel;
use crate::database::table::table::Table;
use crate::source::ani_list_source::AniListSource;
use crate::{Config, database::database::Database, source::manga_dex_source::MangaDexSource};

struct Data {
    config: &'static Config,
    db: Arc<Database>,
    mangadex_source: MangaDexSource,
    anilist_source: AniListSource,
}

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
async fn subscribe(
    ctx: Context<'_>,
    #[description = "Type of series"] series_type: SeriesType,
    #[description = "ID of the series"] series_id: String,
    #[description = "Where to send the notifications"] send_into: SendInto,
) -> anyhow::Result<(), Error> {
    let user_id = ctx.author().id.to_string();
    let data = ctx.data();

    let series_latest: String;
    let series_published: DateTime<Utc>;

    match series_type {
        SeriesType::Manga => {
            if let Some(res) = ctx.data().mangadex_source.get_latest(&series_id).await? {
                series_latest = res.chapter_id;
                series_published = res.published;
            } else {
                ctx.say(format!(
                    "❌ Manga with series id {} not found on MangaDex",
                    series_id
                ))
                .await?;
                return Ok(());
            }
        }
        SeriesType::Anime => {
            if let Some(res) = ctx.data().anilist_source.get_latest(&series_id).await? {
                series_latest = res.episode_id;
                series_published = res.published;
            } else {
                ctx.say(format!(
                    "❌ Anime with series id {} not found on AniList",
                    series_id
                ))
                .await?;
                return Ok(());
            }
        }
    }

    let latest_update = LatestUpdatesModel {
        id: 0,
        r#type: series_type.as_str().to_string(),
        series_id: series_id.clone(),
        series_latest: series_latest,
        series_published: series_published,
    };
    let latest_update_id = data
        .db
        .latest_updates_table
        .insert(&latest_update)
        .await?;

    let subscriber = SubscribersModel {
        id: 0,
        subscriber_id: {
            if send_into.as_str() == "webhook" {
                ctx.data().config.webhook_url.clone()
            } else {
                user_id
            }
        },
        subscriber_type: send_into.as_str().to_string(),
        latest_updates_id: latest_update_id,
    };
    ctx.data().db.subscribers_table.insert(&subscriber).await?;

    ctx.say(format!(
        "✅ Successfully subscribed to {} series `{}`",
        series_type.as_str(),
        series_id
    ))
    .await?;

    Ok(())
}

#[poise::command(slash_command)]
async fn unsubscribe(
    ctx: Context<'_>,
    #[description = "Type of series"] series_type: SeriesType,
    #[description = "ID of the series"] series_id: String,
    #[description = "Where the notifications were sent"] send_into: SendInto,
) -> anyhow::Result<(), Error> {
    let user_id = ctx.author().id.to_string();
    let data = ctx.data();

    let latest_update_id = LatestUpdatesModel {
        id: 0,
        r#type: series_type.as_str().to_string(),
        series_id: series_id.clone(),
        series_latest: "".to_string(),
        series_published: Utc::now(),
    };

    let latest_update_id = if let Ok(res) = data
        .db
        .latest_updates_table
        .select_by_model(latest_update_id)
        .await
    {
        res.id
    } else {
        ctx.say("❌ Not found").await?;
        return Ok(());
    };

    let subscriber = SubscribersModel {
        id: 0,
        subscriber_type: send_into.as_str().to_string(),
        subscriber_id: {
            if send_into.as_str() == "webhook" {
                ctx.data().config.webhook_url.clone()
            } else {
                user_id
            }
        },
        latest_updates_id: latest_update_id,
    };

    if data
        .db
        .subscribers_table
        .delete_by_model(subscriber)
        .await
        .is_err()
    {
        ctx.say("❌ Can't delete from 'subscribers' table").await?;
        return Ok(());
    }

    ctx.say(format!(
        "✅ Successfully unsubscribed from {} series `{}`",
        series_type.as_str(),
        series_id
    ))
    .await?;

    Ok(())
}

#[poise::command(slash_command)]
async fn help(
    ctx: Context<'_>,
    #[description = "Specific command to show help about"]
    #[autocomplete = "poise::builtins::autocomplete_command"]
    command: Option<String>,
) -> anyhow::Result<(), Error> {
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

// async fn get_client(config: Arc<Config>, listener: Arc<RwLock<PollingListener>>) {
//     let config_clone = Arc::clone(&config);
//     let framework = poise::Framework::builder()
//         .options(poise::FrameworkOptions {
//             commands: vec![subscribe(), unsubscribe(), help()],
//             prefix_options: poise::PrefixFrameworkOptions {
//                 prefix: Some("!".into()),
//                 edit_tracker: Some(Arc::new(poise::EditTracker::for_timespan(
//                     std::time::Duration::from_secs(3600),
//                 ))),
//                 ..Default::default()
//             },
//             ..Default::default()
//         })
//         .setup(|ctx, _ready, framework| {
//             Box::pin(async move {
//                 poise::builtins::register_globally(ctx, &framework.options().commands).await?;
//                 Ok(Data {
//                     listener,
//                     config: config_clone
//                 })
//             })
//         })
//         .build();
//
//     let intents = serenity::GatewayIntents::non_privileged() | serenity::GatewayIntents::MESSAGE_CONTENT;
//     let client = serenity::ClientBuilder::new(&config.discord_token, intents).framework(framework).await;
//     client.unwrap().start().await.unwrap();
// }
