//! Discord bot implementation and command handling.

pub mod checks;
pub mod commands;
pub mod controller;
pub mod error;
pub mod error_handler;
pub mod navigation;
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
use serenity::all::ActivityData;
use serenity::all::ApplicationId;
use serenity::all::FullEvent;
use serenity::all::GuildId;
use serenity::all::Token;
use serenity::small_fixed_array::FixedString;

type Error = Box<dyn std::error::Error + Send + Sync>;

use crate::bot::commands::Cog;
use crate::bot::commands::Cogs;
use crate::bot::error_handler::ErrorHandler;
use crate::config::Config;
use crate::event::VoiceStateEvent;
use crate::event::event_bus::EventBus;
use crate::feed::platforms::Platforms;
use crate::model::BotMetaKey;
use crate::service::Services;
use crate::subscriber::voice_state_subscriber::VoiceStateSubscriber;

/// Data shared across bot commands and contexts.
pub struct Data {
    pub config: Arc<Config>,
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
        event_bus: Arc<EventBus>,
        platforms: Arc<Platforms>,
        service: Arc<Services>,
        voice_subscriber: Arc<VoiceStateSubscriber>,
    ) -> Result<Self> {
        info!("Initializing bot...");

        let (token, intents) = Self::create_client_config(&config)?;
        let framework = Self::create_framework(&config)?;
        let http = Http::new(token.clone());
        if let Some(application_id) = config.discord_application_id {
            http.set_application_id(ApplicationId::new(application_id));
        }
        let http = Arc::new(http);
        let data = Arc::new(Data {
            config: config.clone(),
            platforms,
            service,
            start_time: Instant::now(),
        });

        let event_handler = Arc::new(BotEventHandler::new(
            event_bus,
            data.clone(),
            voice_subscriber.clone(),
            http.clone(),
        ));

        let client_builder = ClientBuilder::new(token.clone(), intents)
            .event_handler(event_handler)
            .framework(framework)
            .data(data)
            .activity(ActivityData::playing(format!(
                "v{}",
                config.version.clone()
            )));

        Ok(Self {
            cache: Arc::new(Cache::default()),
            http,
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
        let options = FrameworkOptions::<Data, Error> {
            commands: Cogs.commands(),
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

    async fn on_error(error: poise::FrameworkError<'_, Data, Error>) {
        ErrorHandler::handle(error).await;
    }
}

/// Event handler for Discord gateway events.
pub struct BotEventHandler {
    event_bus: Arc<EventBus>,
    data: Arc<Data>,
    voice_subscriber: Arc<VoiceStateSubscriber>,
    http: Arc<poise::serenity_prelude::Http>,
}

impl BotEventHandler {
    pub fn new(
        event_bus: Arc<EventBus>,
        data: Arc<Data>,
        voice_subscriber: Arc<VoiceStateSubscriber>,
        http: Arc<poise::serenity_prelude::Http>,
    ) -> Self {
        Self {
            event_bus,
            data,
            voice_subscriber,
            http,
        }
    }

    /// Scans all guilds for users currently in voice channels.
    async fn scan_voice_channels(&self, ctx: &poise::serenity_prelude::Context) {
        let mut tracked = 0u32;
        let guild_ids: Vec<_> = ctx.cache.guilds().into_iter().collect();

        for guild_id in guild_ids {
            let is_enabled = self
                .data
                .service
                .voice_tracking
                .is_enabled(guild_id.get())
                .await;

            if !is_enabled {
                continue;
            }

            let voice_states = self.collect_voice_states(ctx, guild_id);

            for (user_id, guild_id, channel_id, session_id) in voice_states {
                match self
                    .voice_subscriber
                    .track_existing_user(user_id, guild_id, channel_id, &session_id)
                    .await
                {
                    Ok(_) => tracked += 1,
                    Err(e) => debug!(
                        "Failed to track existing user {} in guild {}: {}",
                        user_id, guild_id, e
                    ),
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

    /// Collects voice state data from the cache for a guild.
    /// Returns early with an empty vec if the guild is not cached.
    fn collect_voice_states(
        &self,
        ctx: &poise::serenity_prelude::Context,
        guild_id: GuildId,
    ) -> Vec<(u64, u64, u64, FixedString)> {
        let guild = match ctx.cache.guild(guild_id) {
            Some(g) => g,
            None => return vec![],
        };

        guild
            .voice_states
            .iter()
            .filter_map(|voice_state| {
                let channel_id = voice_state.channel_id?;
                let user_id = voice_state.user_id;

                let is_bot = guild
                    .members
                    .get(&user_id)
                    .map(|m| m.user.bot())
                    .unwrap_or(false);

                if is_bot {
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
    }

    /// Registers commands globally if the bot version has changed.
    async fn register_commands_if_needed(&self) {
        if !self.data.config.features.autoregister_cmds {
            info!(
                "Autoregister command feature is disabled. Commands will not be registered globally."
            );
            return;
        }

        let current_version = self.data.config.version.clone();
        let service = self.data.service.internal.clone();

        // Get stored version from database
        let stored_version = service.get_meta(BotMetaKey::BotVersion).await;

        match stored_version {
            Ok(Some(version)) if version == current_version => {
                debug!("Bot version unchanged ({})", current_version);
            }
            _ => {
                // Version mismatch or not found - register commands globally
                info!(
                    "Bot version changed or not found. Registering commands globally (current: {}, stored: {:?})",
                    current_version,
                    stored_version.ok().flatten()
                );

                let commands = Cogs.commands();
                match poise::builtins::register_globally(&self.http, &commands).await {
                    Ok(_) => {
                        info!("Commands registered globally successfully");

                        // Update stored version
                        if let Err(e) = service.set_meta("bot_version", current_version).await {
                            error!("Failed to update bot version in database: {}", e);
                        }
                    }
                    Err(e) => {
                        error!("Failed to register commands globally: {}", e);
                    }
                }
            }
        }
    }
}

#[async_trait]
impl poise::serenity_prelude::EventHandler for BotEventHandler {
    async fn dispatch(&self, ctx: &poise::serenity_prelude::Context, event: &FullEvent) {
        match event {
            FullEvent::Ready { .. } => {
                info!("Bot is ready, scanning voice channels...");
                self.scan_voice_channels(ctx).await;

                // Check if commands need to be re-registered
                self.register_commands_if_needed().await;
            }
            FullEvent::VoiceStateUpdate { old, new, .. } => {
                self.event_bus.publish(VoiceStateEvent {
                    old: old.clone(),
                    new: new.clone(),
                });
            }
            _ => {}
        }
    }
}
