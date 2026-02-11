//! Discord bot implementation and command handling.

pub mod checks;
pub mod commands;
pub mod error;
pub mod error_handler;
pub mod utils;
pub mod views;

use std::collections::HashSet;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use anyhow;
use anyhow::Result;
use async_trait::async_trait;
use futures::lock::Mutex;
use log::debug;
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
use crate::subscriber::voice_state_subscriber::VoiceStateSubscriber;

/// Data shared across bot commands and contexts.
pub struct Data {
    pub config: Arc<Config>,
    pub db: Arc<Database>,
    pub platforms: Arc<Platforms>,
    pub service: Arc<Services>,
    pub start_time: Instant,
}

/// Discord bot client and framework.
pub struct Bot {
    pub cache: Arc<Cache>,
    pub http: Arc<Http>,
    client_builder: Option<ClientBuilder>,
    client: Arc<Mutex<Option<Client>>>,
}

impl Bot {
    /// Creates a new bot instance with all required components.
    pub async fn new(
        config: Arc<Config>,
        db: Arc<Database>,
        event_bus: Arc<EventBus>,
        platforms: Arc<Platforms>,
        service: Arc<Services>,
        voice_subscriber: Arc<VoiceStateSubscriber>,
    ) -> Result<Self> {
        info!("Initializing bot...");

        let framework = Self::create_framework(&config)?;
        let data = Arc::new(Data {
            config: config.clone(),
            db,
            platforms,
            service,
            start_time: Instant::now(),
        });
        let (token, intents) = Self::create_client_config(&config)?;
        let event_handler = Arc::new(BotEventHandler::new(event_bus, voice_subscriber));

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

    /// Starts the bot client in a background task.
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

    /// Creates the Poise framework with commands and configuration.
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

    /// Creates Discord client configuration (token and intents).
    fn create_client_config(config: &Config) -> Result<(Token, GatewayIntents)> {
        let token = Token::from_str(&config.discord_token)?;
        let intents = GatewayIntents::non_privileged() | GatewayIntents::MESSAGE_CONTENT;
        Ok((token, intents))
    }

    /// Handles framework errors by delegating to the error handler.
    async fn on_error(error: poise::FrameworkError<'_, Data, Error>) {
        ErrorHandler::handle(error).await;
    }
}

/// Event handler for Discord gateway events.
pub struct BotEventHandler {
    event_bus: Arc<EventBus>,
    voice_subscriber: Arc<VoiceStateSubscriber>,
}

impl BotEventHandler {
    /// Creates a new event handler with the event bus and voice subscriber.
    pub fn new(event_bus: Arc<EventBus>, voice_subscriber: Arc<VoiceStateSubscriber>) -> Self {
        Self {
            event_bus,
            voice_subscriber,
        }
    }

    /// Scan all guilds for users currently in voice channels
    async fn scan_voice_channels(&self, ctx: &poise::serenity_prelude::Context) {
        let mut tracked = 0u32;

        // Collect guild IDs first to avoid holding cache references across await
        let guild_ids: Vec<_> = ctx.cache.guilds().into_iter().collect();

        for guild_id in guild_ids {
            // Check if voice tracking is enabled for this guild (before accessing cache)
            let is_enabled = self
                .voice_subscriber
                .services
                .voice_tracking
                .is_enabled(guild_id.get())
                .await;

            if !is_enabled {
                continue;
            }

            // Collect voice state data from cache without holding reference across await
            let voice_states_to_track: Vec<_> = {
                let guild = match ctx.cache.guild(guild_id) {
                    Some(g) => g,
                    None => continue,
                };

                guild
                    .voice_states
                    .iter()
                    .filter_map(|voice_state| {
                        let channel_id = voice_state.channel_id?;
                        let user_id = voice_state.user_id;

                        // Skip bots
                        if let Some(member) = guild.members.get(&user_id)
                            && member.user.bot()
                        {
                            return None;
                        }

                        Some((
                            user_id.get(),
                            guild_id.get(),
                            channel_id.get(),
                            voice_state.session_id.clone(),
                        ))
                    })
                    .collect()
            };

            // Process the collected data (can use await here)
            for (user_id, guild_id, channel_id, session_id) in voice_states_to_track {
                if let Err(e) = self
                    .voice_subscriber
                    .track_existing_user(user_id, guild_id, channel_id, &session_id)
                    .await
                {
                    debug!(
                        "Failed to track existing user {} in guild {}: {}",
                        user_id, guild_id, e
                    );
                } else {
                    tracked += 1;
                }
            }
        }

        if tracked > 0 {
            info!(
                "Voice channel scan complete: {} users now being tracked",
                tracked
            );
        }
    }
}

#[async_trait]
impl poise::serenity_prelude::EventHandler for BotEventHandler {
    async fn dispatch(&self, _context: &poise::serenity_prelude::Context, _event: &FullEvent) {
        match _event {
            FullEvent::Ready { .. } => {
                info!("Bot is ready, scanning voice channels...");
                self.scan_voice_channels(_context).await;
            }
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
