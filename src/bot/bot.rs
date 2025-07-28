use anyhow;
use anyhow::Result;
use poise::serenity_prelude as serenity;
use std::sync::Arc;
use std::time::Duration;

use super::commands::{help, register, subscribe, unsubscribe};
use crate::source::ani_list_source::AniListSource;
use crate::{Config, database::database::Database, source::manga_dex_source::MangaDexSource};

pub struct Data {
    pub config: Arc<Config>,
    pub db: Arc<Database>,
    pub mangadex_source: MangaDexSource,
    pub anilist_source: AniListSource,
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
