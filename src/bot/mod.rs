//! Discord bot implementation and command handling.
//!
//! This module contains the main [`Bot`] struct which manages the Discord client,
//! and the [`BotEventHandler`] which processes gateway events. It acts as the
//! bridge between the Discord gateway and the application's internal services.

pub mod checks;
pub mod command;
pub mod error;
pub mod error_handler;
pub mod host_ctx;
pub mod navigation;
pub mod plugin;
pub mod test_framework;
pub mod utils;
pub mod view;

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
use poise::serenity_prelude::*;

type Error = Box<dyn std::error::Error + Send + Sync>;

use crate::bot::command::Cog;
use crate::bot::command::Cogs;
use crate::bot::error_handler::ErrorHandler;
use crate::bot::plugin::registry::PluginRegistry;
use crate::config::Config;
use crate::entity::BotMetaKey;
use crate::event::VoiceStateEvent;
use crate::event::event_bus::EventBus;
use crate::service::Services;

/// Data shared across bot commands and contexts.
pub struct Data {
    pub config: Arc<Config>,
    pub service: Arc<Services>,
    pub event_bus: Arc<EventBus>,
    pub plugin_registry: Arc<PluginRegistry>,
    pub start_time: Instant,
}

/// Discord bot client and framework.
pub struct Bot {
    pub cache: Arc<Cache>,
    pub http: Arc<Http>,
    pub data: Arc<Data>,
    client_builder: Option<ClientBuilder>,
    client: Arc<Mutex<Option<Client>>>,
}

impl Bot {
    /// Creates a new bot instance with all required components.
    pub async fn new(
        config: Arc<Config>,
        event_bus: Arc<EventBus>,
        service: Arc<Services>,
        plugin_registry: Arc<PluginRegistry>,
    ) -> Result<Self> {
        info!("Initializing bot...");

        let (token, intents) = Self::create_client_config(&config)?;
        let framework = Self::create_framework(&config, &plugin_registry)?;
        let http = Http::new(token.clone());
        if let Some(application_id) = config.discord_application_id {
            http.set_application_id(ApplicationId::new(application_id));
        }
        let http = Arc::new(http);
        let data = Arc::new(Data {
            config: config.clone(),
            service,
            event_bus: event_bus.clone(),
            plugin_registry,
            start_time: Instant::now(),
        });

        let event_handler = Arc::new(BotEventHandler::new(event_bus, data.clone(), http.clone()));

        let client_builder = ClientBuilder::new(token.clone(), intents)
            .event_handler(event_handler)
            .framework(framework)
            .data(data.clone())
            .activity(ActivityData::playing(format!(
                "v{}",
                config.version.clone()
            )));

        Ok(Self {
            cache: Arc::new(Cache::default()),
            http,
            data,
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
    fn create_framework(
        config: &Config,
        registry: &PluginRegistry,
    ) -> Result<Box<Framework<Data, Error>>> {
        let mut core_commands = Cogs.commands();
        core_commands.extend(registry.all_commands());
        let options = FrameworkOptions::<Data, Error> {
            commands: core_commands,
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
    http: Arc<poise::serenity_prelude::Http>,
}

impl BotEventHandler {
    pub fn new(
        event_bus: Arc<EventBus>,
        data: Arc<Data>,
        http: Arc<poise::serenity_prelude::Http>,
    ) -> Self {
        Self {
            event_bus,
            data,
            http,
        }
    }

    /// Scans all guilds for users currently in voice channels.
    /// Publishes a synthetic VoiceStateEvent for each active user so that
    /// the voice plugin can start tracking them.
    async fn scan_voice_channels(&self, ctx: &poise::serenity_prelude::Context) {
        let mut tracked = 0u32;
        let guild_ids: Vec<_> = ctx.cache.guilds().into_iter().collect();

        for guild_id in guild_ids {
            let voice_states = {
                let Some(guild) = ctx.cache.guild(guild_id) else {
                    continue;
                };
                self.collect_voice_states_from_guild(&guild)
            };

            for (user_id, guild_id, channel_id, session_id) in &voice_states {
                let vs = Self::make_voice_state(
                    *user_id,
                    Some(*guild_id),
                    Some(*channel_id),
                    session_id.as_str(),
                );
                let event = VoiceStateEvent { old: None, new: vs };
                if let Ok(json) = serde_json::to_value(&event) {
                    let _ = self.event_bus.publish_named("voice_state", json);
                }
                tracked += 1;
            }
        }

        if tracked > 0 {
            info!("Voice channel scan complete: {tracked} users now being tracked");
        }
    }

    /// Collects voice state data from a guild reference.
    /// For large guilds the member list may be incomplete on `GuildCreate`; in that case
    /// we default to treating unknown users as non-bots (better to over-track than under-track).
    /// Constructs a minimal VoiceState from component values.
    fn make_voice_state(
        user_id: u64,
        guild_id: Option<u64>,
        channel_id: Option<u64>,
        session_id: &str,
    ) -> poise::serenity_prelude::VoiceState {
        serde_json::from_value(serde_json::json!({
            "user_id": user_id.to_string(),
            "guild_id": guild_id.map(|id| id.to_string()),
            "channel_id": channel_id.map(|id| id.to_string()),
            "session_id": session_id,
            "deaf": false,
            "mute": false,
            "self_deaf": false,
            "self_mute": false,
            "suppress": false,
            "self_video": false,
        }))
        .expect("Minimal VoiceState construction should never fail")
    }

    fn collect_voice_states_from_guild(
        &self,
        guild: &Guild,
    ) -> Vec<(u64, u64, u64, small_fixed_array::FixedString)> {
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
                    guild.id.get(),
                    channel_id.get(),
                    voice_state.session_id.clone(),
                ))
            })
            .collect()
    }

    /// Registers commands globally if the bot version has changed.
    async fn register_commands_if_needed(&self) {
        if !self.data.config.features.is_enabled("autoregister_cmds") {
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
                debug!("Bot version unchanged ({current_version})");
            }
            _ => {
                // Version mismatch or not found - register commands globally
                info!(
                    "Bot version changed or not found. Registering commands globally (current: {}, stored: {:?})",
                    current_version,
                    stored_version.ok().flatten()
                );

                let mut commands = Cogs.commands();
                commands.extend(self.data.plugin_registry.all_commands());
                match poise::builtins::register_globally(&self.http, &commands).await {
                    Ok(_) => {
                        info!("Commands registered globally successfully");

                        // Update stored version
                        if let Err(e) = service
                            .set_meta(BotMetaKey::BotVersion, current_version)
                            .await
                        {
                            error!("Failed to update bot version in database: {e}");
                        }
                    }
                    Err(e) => {
                        error!("Failed to register commands globally: {e}");
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
            FullEvent::GuildCreate { guild, .. } => {
                let voice_states = self.collect_voice_states_from_guild(guild);
                let mut tracked = 0u32;

                for (user_id, guild_id, channel_id, session_id) in &voice_states {
                    let vs = Self::make_voice_state(
                        *user_id,
                        Some(*guild_id),
                        Some(*channel_id),
                        session_id.as_str(),
                    );
                    let event = VoiceStateEvent { old: None, new: vs };
                    if let Ok(json) = serde_json::to_value(&event) {
                        let _ = self.event_bus.publish_named("voice_state", json);
                    }
                    tracked += 1;
                }

                if tracked > 0 {
                    info!(
                        "Guild {} scan complete: {} users now being tracked",
                        guild.id.get(),
                        tracked
                    );
                }
            }
            FullEvent::VoiceStateUpdate { old, new, .. } => {
                let event = VoiceStateEvent {
                    old: old.clone(),
                    new: new.clone(),
                };
                // Keep the typed publish for any remaining core subscribers
                self.event_bus.publish(event.clone());
                // Also publish as named event for plugins
                if let Ok(json) = serde_json::to_value(&event) {
                    let _ = self.event_bus.publish_named("voice_state", json);
                }
            }
            _ => {}
        }
    }
}
