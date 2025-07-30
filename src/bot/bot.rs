use anyhow;
use anyhow::Result;
use futures::lock::Mutex;
use log::info;
use poise::serenity_prelude as serenity;
use std::sync::Arc;
use std::time::Duration;

use super::commands::{help, register, subscribe, unsubscribe};
use crate::source::ani_list_source::AniListSource;
use crate::{
    config::Config, database::database::Database, source::manga_dex_source::MangaDexSource,
};

pub struct Data {
    pub config: Arc<Config>,
    pub db: Arc<Database>,
    pub manga_source: Arc<MangaDexSource>,
    pub anime_source: Arc<AniListSource>,
}

pub struct Bot {
    client: Arc<Mutex<serenity::Client>>,
    pub cache: Arc<serenity::Cache>,
    pub http: Arc<serenity::Http>,
}

impl Bot {
    pub async fn new(
        config: Arc<Config>,
        db: Arc<Database>,
        anime_source: Arc<AniListSource>,
        manga_source: Arc<MangaDexSource>,
    ) -> Result<Self> {
        info!("Initializing bot...");
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
            db: db.clone(),
            manga_source: manga_source.clone(),
            anime_source: anime_source.clone(),
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

        Ok(Self {
            cache: client.cache.clone(),
            http: client.http.clone(),
            client: Arc::new(Mutex::new(client)),
        })
    }

    pub fn start(&mut self) {
        info!("Starting bot client...");
        let client = self.client.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                client
                    .lock()
                    .await
                    .start()
                    .await
                    .expect("Failed to start bot client");
            })
        });
        info!("Bot client started.");
    }
}
