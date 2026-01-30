pub mod checks;
pub mod commands;
pub mod error;
pub mod error_handler;
pub mod views;

use std::collections::HashSet;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use anyhow;
use anyhow::Result;
use async_trait::async_trait;
use futures::lock::Mutex;
use log::info;
use poise::Framework;
use poise::FrameworkOptions;
use poise::serenity_prelude::Cache;
use poise::serenity_prelude::Client;
use poise::serenity_prelude::ClientBuilder;
use poise::serenity_prelude::GatewayIntents;
use poise::serenity_prelude::Http;
use poise::serenity_prelude::UserId;
use serenity::all::FullEvent;
use serenity::all::Token;

type Error = Box<dyn std::error::Error + Send + Sync>;

use crate::bot::commands::Cog;
use crate::bot::commands::Cogs;
use crate::bot::error_handler::ErrorHandler;
use crate::config::Config;
use crate::database::Database;
use crate::event::VoiceStateEvent;
use crate::event::event_bus::EventBus;
use crate::feed::platforms::Platforms;
use crate::service::Services;

pub struct Data {
    pub config: Arc<Config>,
    pub db: Arc<Database>,
    pub platforms: Arc<Platforms>,
    pub service: Arc<Services>,
}

pub struct Bot {
    pub cache: Arc<Cache>,
    pub http: Arc<Http>,
    client_builder: Option<ClientBuilder>,
    client: Arc<Mutex<Option<Client>>>,
}

impl Bot {
    pub async fn new(
        config: Arc<Config>,
        db: Arc<Database>,
        event_bus: Arc<EventBus>,
        platforms: Arc<Platforms>,
        service: Arc<Services>,
    ) -> Result<Self> {
        info!("Initializing bot...");

        let framework = Self::create_framework(&config)?;
        let data = Arc::new(Data {
            config: config.clone(),
            db,
            platforms,
            service,
        });
        let (token, intents) = Self::create_client_config(&config)?;
        let event_handler = Arc::new(BotEventHandler::new(event_bus));

        let client_builder = ClientBuilder::new(token.clone(), intents)
            .event_handler(event_handler)
            .framework(framework)
            .data(data);

        Ok(Self {
            cache: Arc::new(Cache::default()),
            http: Arc::new(Http::new(token)),
            client_builder: Some(client_builder),
            client: Arc::new(Mutex::new(None)),
        })
    }

    pub fn start(&mut self) {
        info!("Starting bot client...");
        let client_builder = self.client_builder.take().expect("start() called twice");
        let client = self.client.clone();

        tokio::spawn(async move {
            info!("Connecting bot to Discord...");
            let built_client = client_builder
                .await
                .expect("Failed to build Discord client");

            *client.lock().await = Some(built_client);
            info!("Bot connected to Discord.");

            client
                .lock()
                .await
                .as_mut()
                .unwrap()
                .start()
                .await
                .expect("Bot client crashed");
        });

        info!("Bot client start initiated.");
    }

    fn create_framework(config: &Config) -> Result<Box<Framework<Data, Error>>> {
        let cogs = Cogs;
        let options = FrameworkOptions::<Data, Error> {
            commands: cogs.commands(),
            on_error: |error| Box::pin(Self::on_error(error)),
            prefix_options: poise::PrefixFrameworkOptions {
                prefix: Some("!".into()),
                edit_tracker: Some(Arc::new(poise::EditTracker::for_timespan(
                    Duration::from_secs(3600),
                ))),
                ..Default::default()
            },
            owners: HashSet::from([UserId::from_str(&config.admin_id)
                .map_err(|_| anyhow::anyhow!("Invalid admin ID"))?]),
            ..Default::default()
        };

        Ok(Box::new(
            poise::Framework::builder().options(options).build(),
        ))
    }

    fn create_client_config(config: &Config) -> Result<(Token, GatewayIntents)> {
        let token = Token::from_str(&config.discord_token)?;
        let intents = GatewayIntents::non_privileged() | GatewayIntents::MESSAGE_CONTENT;
        Ok((token, intents))
    }

    async fn on_error(error: poise::FrameworkError<'_, Data, Error>) {
        ErrorHandler::handle(error).await;
    }
}

pub struct BotEventHandler {
    event_bus: Arc<EventBus>,
}

impl BotEventHandler {
    pub fn new(event_bus: Arc<EventBus>) -> Self {
        Self { event_bus }
    }
}

#[async_trait]
impl poise::serenity_prelude::EventHandler for BotEventHandler {
    async fn dispatch(&self, _context: &poise::serenity_prelude::Context, _event: &FullEvent) {
        #[allow(clippy::single_match)]
        match _event {
            FullEvent::VoiceStateUpdate { old, new, .. } => {
                self.event_bus.publish(VoiceStateEvent {
                    old: old.clone(),
                    new: new.clone(),
                });
            }
            _ => {}
        };
    }
}
