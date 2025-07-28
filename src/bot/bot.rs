use anyhow;
use anyhow::Result;
use chrono::{DateTime, Utc};
use poise::ChoiceParameter;
use poise::serenity_prelude as serenity;
use std::sync::Arc;
use std::time::Duration;

use crate::database::model::latest_updates_model::LatestUpdatesModel;
use crate::database::model::subscribers_model::SubscribersModel;
use crate::database::table::table::Table;
use crate::source::ani_list_source::AniListSource;
use crate::{Config, database::database::Database, source::manga_dex_source::MangaDexSource};

struct Data {
    config: Arc<Config>,
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

    let series_title: String;
    let series_latest: String;
    let series_published: DateTime<Utc>;

    match series_type {
        SeriesType::Manga => {
            if let Some(res) = data.mangadex_source.get_latest(&series_id).await? {
                series_title = res.title;
                series_latest = res.chapter_id;
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
            if let Some(res) = data.anilist_source.get_latest(&series_id).await? {
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

    let latest_update = LatestUpdatesModel {
        id: 0,
        r#type: series_type.as_str().to_string(),
        series_id: series_id.clone(),
        series_latest: series_latest,
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
        latest_updates_id: latest_update_id,
    };

    if data.db.subscribers_table.insert(&subscriber).await.is_err() {
        ctx.say(format!(
            "You are already subscribed to this {}",
            series_type.as_str()
        ))
        .await?;
        return Ok(());
    };

    ctx.say(format!(
        "✅ Successfully subscribed to \"{}\" series \"{}\"",
        series_type.as_str(),
        series_title
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

    let latest_update = LatestUpdatesModel {
        id: 0,
        r#type: series_type.as_str().to_string(),
        series_id: series_id.clone(),
        series_latest: "".to_string(),
        series_published: Utc::now(),
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
            "❌ type \"{}\" and series_id \"{}\" not found in the database",
            latest_update.r#type, latest_update.series_id
        ))
        .await?;
        return Ok(());
    };

    let subscriber = SubscribersModel {
        id: 0,
        subscriber_type: send_into.as_str().to_string(),
        subscriber_id: {
            if send_into.as_str() == "webhook" {
                data.config.webhook_url.clone()
            } else {
                user_id
            }
        },
        latest_updates_id: latest_update_id,
    };

    data.db
        .subscribers_table
        .delete_by_model(subscriber)
        .await?;

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

#[poise::command(prefix_command)]
async fn register(ctx: Context<'_>) -> Result<(), Error> {
    poise::builtins::register_application_commands_buttons(ctx).await?;
    Ok(())
}

pub struct Bot {
    pub client: serenity::Client,
}

impl Bot {
    pub async fn new(config: Arc<Config>, db: Arc<Database>) -> Result<Self> {
        let options = poise::FrameworkOptions {
            commands: vec![subscribe(), unsubscribe(), help(), register()],
            prefix_options: poise::PrefixFrameworkOptions {
                prefix: Some("!".into()),
                edit_tracker: Some(Arc::new(poise::EditTracker::for_timespan(
                    Duration::from_secs(3600),
                ))),
                ..Default::default()
            },
            ..Default::default()
        };
        let data = Data {
            config: config.clone(),
            db: db,
            mangadex_source: MangaDexSource::new(),
            anilist_source: AniListSource::new(),
        };
        let framework = poise::Framework::builder()
            .options(options)
            .setup(|_ctx, _ready, _framework| Box::pin(async move { Ok(data) }))
            .build();
        let intents =
            serenity::GatewayIntents::non_privileged() | serenity::GatewayIntents::MESSAGE_CONTENT;
        let client = serenity::ClientBuilder::new(&config.discord_token, intents)
            .framework(framework)
            .await?;
        Ok(Self { client })
    }

    pub async fn start(&mut self) -> Result<()> {
        self.client.start().await?;
        Ok(())
    }
}
