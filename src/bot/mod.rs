pub mod commands;

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
use serenity::all::Token;

type Error = Box<dyn std::error::Error + Send + Sync>;

use crate::bot::commands::dump_db;
use crate::bot::commands::register;
use crate::bot::commands::subscribe;
use crate::bot::commands::subscriptions;
use crate::bot::commands::unsubscribe;
use crate::config::Config;
use crate::database::Database;
use crate::feed::feeds::Feeds;

pub struct Data {
    pub config: Arc<Config>,
    pub db: Arc<Database>,
    pub feeds: Arc<Feeds>,
}

pub struct Bot {
    client: Arc<Mutex<Client>>,
    pub cache: Arc<Cache>,
    pub http: Arc<Http>,
}

impl Bot {
    pub async fn new(config: Arc<Config>, db: Arc<Database>, feeds: Arc<Feeds>) -> Result<Self> {
        info!("Initializing bot...");
        let options = FrameworkOptions::<Data, Error> {
            commands: vec![
                subscribe(),
                unsubscribe(),
                subscriptions(),
                dump_db(),
                register(),
            ],
            on_error: |error| Box::pin(Bot::on_error(error)),
            prefix_options: poise::PrefixFrameworkOptions {
                prefix: Some("!".into()),
                edit_tracker: Some(Arc::new(poise::EditTracker::for_timespan(
                    Duration::from_secs(3600),
                ))),
                ..Default::default()
            },
            owners: HashSet::from([
                UserId::from_str(config.admin_id.as_str()).expect("Invalid admin ID")
            ]),
            ..Default::default()
        };
        let data = Arc::new(Data {
            config: config.clone(),
            db: db.clone(),
            feeds: feeds.clone(),
        });

        let intents = GatewayIntents::non_privileged() | GatewayIntents::MESSAGE_CONTENT;
        let token = Token::from_str(&config.discord_token)?;

        let framework: Box<Framework<Data, Error>> =
            Box::new(poise::Framework::builder().options(options).build());

        let client = ClientBuilder::new(token, intents)
            .framework(framework)
            .data(data)
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
