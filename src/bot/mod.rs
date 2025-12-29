pub mod cog;
pub mod components;
pub mod error;

use std::collections::HashSet;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use anyhow;
use anyhow::Result;
use futures::lock::Mutex;
use log::error;
use log::info;
use poise::Framework;
use poise::FrameworkOptions;
use poise::serenity_prelude::Cache;
use poise::serenity_prelude::Client;
use poise::serenity_prelude::ClientBuilder;
use poise::serenity_prelude::GatewayIntents;
use poise::serenity_prelude::Http;
use poise::serenity_prelude::UserId;
use serenity::all::CreateComponent;
use serenity::all::CreateContainer;
use serenity::all::CreateContainerComponent;
use serenity::all::CreateTextDisplay;
use serenity::all::MessageFlags;
use serenity::all::Token;

type Error = Box<dyn std::error::Error + Send + Sync>;

use crate::bot::cog::feeds_cog::FeedsCog;
use crate::bot::cog::owner_cog::OwnerCog;
use crate::config::Config;
use crate::database::Database;
use crate::feed::feeds::Feeds;
use crate::service::feed_subscription_service::FeedSubscriptionService;

pub struct Data {
    pub config: Arc<Config>,
    pub db: Arc<Database>,
    pub feeds: Arc<Feeds>,
    pub feed_subscription_service: Arc<FeedSubscriptionService>,
}

pub struct Bot {
    pub cache: Arc<Cache>,
    pub http: Arc<Http>,
    client_builder: Option<ClientBuilder>,
    client: Arc<Mutex<Option<Client>>>,
}

impl Bot {
    pub async fn new(config: Arc<Config>, db: Arc<Database>, feeds: Arc<Feeds>) -> Result<Self> {
        info!("Initializing bot...");

        let framework = Self::create_framework(&config)?;
        let data = Self::create_data(config.clone(), db, feeds);
        let (token, intents) = Self::create_client_config(&config)?;

        let client_builder = ClientBuilder::new(token.clone(), intents)
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
            match client_builder.await {
                Ok(built_client) => {
                    *client.lock().await = Some(built_client);

                    if let Err(e) = client.lock().await.as_mut().unwrap().start().await {
                        error!("Bot client crashed: {}", e);
                    }
                    info!("Bot connected to Discord.");
                }
                Err(e) => {
                    error!("Failed to build client: {}", e);
                }
            }
        });

        info!("Bot client start initiated.");
    }

    fn create_framework(config: &Config) -> Result<Box<Framework<Data, Error>>> {
        let options = FrameworkOptions::<Data, Error> {
            commands: vec![
                FeedsCog::settings(),
                FeedsCog::subscribe(),
                FeedsCog::unsubscribe(),
                FeedsCog::subscriptions(),
                OwnerCog::dump_db(),
                OwnerCog::register(),
            ],
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

    fn create_data(config: Arc<Config>, db: Arc<Database>, feeds: Arc<Feeds>) -> Arc<Data> {
        let feed_subscription_service = Arc::new(FeedSubscriptionService {
            db: db.clone(),
            feeds: feeds.clone(),
        });

        Arc::new(Data {
            config,
            db,
            feeds,
            feed_subscription_service,
        })
    }

    fn create_client_config(config: &Config) -> Result<(Token, GatewayIntents)> {
        let token = Token::from_str(&config.discord_token)?;
        let intents = GatewayIntents::non_privileged() | GatewayIntents::MESSAGE_CONTENT;
        Ok((token, intents))
    }

    async fn on_error(error: poise::FrameworkError<'_, Data, Error>) {
        match error {
            poise::FrameworkError::Command { error, ctx, .. } => {
                error!("Error in command `{}`: {:?}", ctx.command().name, error);

                let error_message = format!(
                    "## ❌ Command Error\n\n**Command:** `{}`\n**Error:** {}",
                    ctx.command().name,
                    error
                );

                let components = vec![CreateComponent::Container(CreateContainer::new(vec![
                    CreateContainerComponent::TextDisplay(CreateTextDisplay::new(error_message)),
                ]))];

                let _ = ctx
                    .send(
                        poise::CreateReply::default()
                            .flags(MessageFlags::IS_COMPONENTS_V2)
                            .components(components),
                    )
                    .await;
            }
            poise::FrameworkError::ArgumentParse { error, ctx, .. } => {
                error!(
                    "Argument parse error in `{}`: {}",
                    ctx.command().name,
                    error
                );

                let error_message = format!(
                    "## ⚠️ Invalid Arguments\n\n**Command:** `{}`\n**Issue:** {}\n\n> Use `/help {}` for usage information.",
                    ctx.command().name,
                    error,
                    ctx.command().name
                );

                let components = vec![CreateComponent::Container(CreateContainer::new(vec![
                    CreateContainerComponent::TextDisplay(CreateTextDisplay::new(error_message)),
                ]))];

                let _ = ctx
                    .send(
                        poise::CreateReply::default()
                            .flags(MessageFlags::IS_COMPONENTS_V2)
                            .components(components),
                    )
                    .await;
            }
            error => {
                if let Err(e) = poise::builtins::on_error(error).await {
                    error!("Error while handling error: {}", e);
                }
            }
        }
    }
}
