use std::collections::HashSet;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use ::serenity::all::UserId;
use anyhow;
use anyhow::Result;
use futures::lock::Mutex;
use log::error;
use log::info;
use poise::serenity_prelude as serenity;

type Error = Box<dyn std::error::Error + Send + Sync>;

use super::commands::dump_db;
use super::commands::help;
use super::commands::register;
use super::commands::subscribe;
use super::commands::subscriptions;
use super::commands::unsubscribe;
use crate::config::Config;
use crate::database::database::Database;
use crate::feed::feeds::Feeds;

pub struct Data {
    pub config: Arc<Config>,
    pub db: Arc<Database>,
    pub sources: Arc<Feeds>,
}

pub struct Bot {
    client: Arc<Mutex<serenity::Client>>,
    pub cache: Arc<serenity::Cache>,
    pub http: Arc<serenity::Http>,
}

impl Bot {
    pub async fn new(config: Arc<Config>, db: Arc<Database>, sources: Arc<Feeds>) -> Result<Self> {
        info!("Initializing bot...");
        let options = poise::FrameworkOptions {
            commands: vec![
                subscribe(),
                unsubscribe(),
                subscriptions(),
                dump_db(),
                help(),
                register(),
            ],
            prefix_options: poise::PrefixFrameworkOptions {
                prefix: Some("!".into()),
                edit_tracker: Some(Arc::new(poise::EditTracker::for_timespan(
                    Duration::from_secs(3600),
                ))),
                ..Default::default()
            },
            on_error: |error| Box::pin(Bot::on_error(error)),
            owners: HashSet::from([
                UserId::from_str(config.admin_id.as_str()).expect("Invalid admin ID")
            ]),
            ..Default::default()
        };
        let data = Data {
            config: config.clone(),
            db: db.clone(),
            sources: sources.clone(),
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

    /// Global custom error handler
    async fn on_error(error: poise::FrameworkError<'_, Data, Error>) {
        match error {
            poise::FrameworkError::Setup { error, .. } => {
                panic!("Failed to start bot: {:?}", error)
            }
            poise::FrameworkError::Command { error, ctx, .. } => {
                error!("Error in command `{}`: {:?}", ctx.command().name, error,);
                let _ = ctx
                    .say(format!(
                        "❌ An error occurred while executing the command ({error:?})",
                    ))
                    .await;
            }
            error => {
                if let Err(e) = poise::builtins::on_error(error).await {
                    error!("Error while handling error: {}", e)
                }
            }
        }
    }
}
