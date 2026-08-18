//! Discord bot implementation and command handling.
//!
//! This module contains the main [`Bot`] struct which manages the Discord client,
//! and the [`BotEventHandler`] which processes gateway events. It acts as the
//! bridge between the Discord gateway and the application's internal services.

pub mod checks;
pub mod command;
pub mod error;
pub mod error_handler;
pub mod navigation;
pub mod test_framework;
pub mod utils;
pub mod view;

use std::collections::HashMap;
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
use log::warn;
use poise::Framework;
use poise::FrameworkOptions;
use poise::serenity_prelude::*;
use pwr_plugin_protocol::Manifest;
use serde_json::Value;

type Error = Box<dyn std::error::Error + Send + Sync>;

use crate::bot::command::Cog;
use crate::bot::command::Cogs;
use crate::bot::error_handler::ErrorHandler;
use crate::config::Config;
use crate::entity::BotMetaKey;
use crate::event::VoiceStateEvent;
use crate::event::event_bus::EventBus;
use crate::feed::Platforms;
use crate::plugin::CatalogEntry;
use crate::plugin::HostConfig;
use crate::plugin::HostServices;
use crate::plugin::InteractionEngine;
use crate::plugin::InteractionError;
use crate::plugin::PgKvStore;
use crate::plugin::PluginCatalog;
use crate::plugin::PluginEventRouter;
use crate::plugin::PluginManager;
use crate::plugin::RespawnPolicy;
use crate::plugin::RunningPlugin;
use crate::plugin::SerenityHostIo;
use crate::plugin::VOICE_STATE_EVENT;
use crate::plugin::command::PluginRoutes;
use crate::plugin::command::commands_from_manifest;
use crate::plugin::command::register_in_guild;
use crate::plugin::command::routes_from_manifests;
use crate::plugin::interaction::DEFAULT_VIEW_TIMEOUT;
use crate::repo::traits::Repos;
use crate::service::Services;
use crate::subscriber::voice_state::VoiceStateSubscriber;

/// Data shared across bot commands and contexts.
pub struct Data {
    pub config: Arc<Config>,
    pub platforms: Arc<Platforms>,
    pub service: Arc<Services>,
    pub repos: Arc<dyn Repos + Send + Sync>,
    pub plugin_manager: Arc<PluginManager>,
    pub plugin_catalog: Arc<HashMap<String, CatalogEntry>>,
    pub plugin_engine: Arc<InteractionEngine<RunningPlugin>>,
    /// Command-name → plugin-name routes for dispatch; built at startup from
    /// the loaded manifests ([`routes_from_manifests`]).
    pub plugin_routes: Arc<PluginRoutes>,
    /// Manifests of the core plugins spawned at startup, by plugin name.
    pub core_manifests: Arc<HashMap<String, Manifest>>,
    pub start_time: Instant,
}

impl Data {
    /// The names of plugins that auto-enable in every guild: the configured
    /// core plugins plus catalog entries flagged `auto_enable`.
    pub fn auto_enable_plugins(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .config
            .core_plugins
            .iter()
            .map(|spec| spec.name.clone())
            .collect();
        for (name, entry) in self.plugin_catalog.iter() {
            if entry.auto_enable && !names.contains(name) {
                names.push(name.clone());
            }
        }
        names
    }

    /// The manifest for an auto-enabled plugin: the core plugin's manifest
    /// captured at spawn when there is one, else the catalog entry's.
    pub fn manifest_for(&self, name: &str) -> Option<&Manifest> {
        manifest_for(&self.core_manifests, &self.plugin_catalog, name)
    }
}

/// The manifest for `name`: the core plugin's manifest when there is one,
/// else the catalog entry's. Shared by [`Data::manifest_for`] and the
/// `/plugins` toggle union, which look up by plugin name over the same two
/// maps.
pub(crate) fn manifest_for<'a>(
    core_manifests: &'a HashMap<String, Manifest>,
    catalog: &'a HashMap<String, CatalogEntry>,
    name: &str,
) -> Option<&'a Manifest> {
    core_manifests
        .get(name)
        .or_else(|| catalog.get(name).map(|entry| &entry.manifest))
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
        repos: Arc<dyn Repos + Send + Sync>,
        voice_subscriber: Arc<VoiceStateSubscriber>,
    ) -> Result<Self> {
        info!("Initializing bot...");

        let (token, intents) = Self::create_client_config(&config)?;
        // http must exist before the framework: the plugin manager and host
        // services (guild-command cleanup, Discord I/O seam) wrap it.
        let http = Http::new(token.clone());
        if let Some(application_id) = config.discord_application_id {
            http.set_application_id(ApplicationId::new(application_id));
        }
        let http = Arc::new(http);

        let catalog = Self::load_plugin_catalog(&config);
        let plugin_engine = Arc::new(InteractionEngine::<RunningPlugin>::with_timeout(
            DEFAULT_VIEW_TIMEOUT,
        ));
        let plugin_events = Arc::new(PluginEventRouter::new());
        let host_services = Arc::new(HostServices {
            io: Some(Arc::new(SerenityHostIo::new(http.clone()))),
            config: Some(HostConfig::from(&*config)),
            kv: Some(Arc::new(PgKvStore::new(repos.plugin_kv()))),
            engine: Some(plugin_engine.clone()),
        });
        let plugin_manager = Arc::new(
            PluginManager::new(Some(http.clone()), RespawnPolicy::default())
                .with_host_services(host_services)
                .with_event_bus(event_bus.clone())
                .with_event_router(plugin_events.clone()),
        );

        // Core plugins are spawned once at startup with no event
        // subscriptions; per-guild command registration follows in
        // Ready/GuildCreate. A missing binary is not fatal: the bot stays up
        // and the plugin's commands simply stay unregistered.
        let mut manifest_sources: Vec<(String, Option<Manifest>)> = Vec::new();
        for spec in &config.core_plugins {
            match plugin_manager
                .spawn(&spec.name, &spec.path, None, &[], &[])
                .await
            {
                Ok(plugin) => {
                    manifest_sources.push((spec.name.clone(), plugin.manifest().cloned()));
                }
                Err(e) => warn!("failed to spawn core plugin {}: {e}", spec.name),
            }
        }
        let core_manifests = Arc::new(
            manifest_sources
                .iter()
                .filter_map(|(name, manifest)| manifest.clone().map(|m| (name.clone(), m)))
                .collect::<HashMap<_, _>>(),
        );
        let mut route_sources = manifest_sources;
        for (name, entry) in &catalog {
            route_sources.push((name.clone(), Some(entry.manifest.clone())));
        }
        let plugin_routes = Arc::new(routes_from_manifests(route_sources));

        let framework = Self::create_framework(&config, &catalog, &core_manifests)?;

        let data = Arc::new(Data {
            config: config.clone(),
            platforms,
            service,
            repos,
            plugin_manager,
            plugin_catalog: Arc::new(catalog),
            plugin_engine,
            plugin_routes,
            core_manifests,
            start_time: Instant::now(),
        });

        let event_handler = Arc::new(BotEventHandler::new(
            event_bus,
            data.clone(),
            voice_subscriber.clone(),
            http.clone(),
            plugin_events,
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
    ///
    /// The command list is the merge seam: the Cog commands first, then one
    /// routing command per core plugin manifest captured at spawn, then one
    /// routing command per catalog plugin manifest — each group sorted by
    /// plugin name for a stable order across restarts (see
    /// [`plugin_commands`]) — so plugin commands are registered on the
    /// framework before `Framework::builder().build()`.
    fn create_framework(
        config: &Config,
        catalog: &HashMap<String, CatalogEntry>,
        core_manifests: &HashMap<String, Manifest>,
    ) -> Result<Box<Framework<Data, Error>>> {
        let mut commands = Cogs.commands();
        commands.extend(plugin_commands(core_manifests, catalog));

        let options = FrameworkOptions::<Data, Error> {
            commands,
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

    /// Loads the plugin catalog from `plugins.toml`. A missing or invalid
    /// catalog logs a warning and yields an empty map: the bot stays up with
    /// plugin commands absent rather than failing startup.
    fn load_plugin_catalog(config: &Config) -> HashMap<String, CatalogEntry> {
        match PluginCatalog::load(&config.plugins_toml) {
            Ok(catalog) => catalog,
            Err(e) => {
                warn!(
                    "failed to load plugin catalog `{}`: {e}",
                    config.plugins_toml.display()
                );
                HashMap::new()
            }
        }
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

/// The plugin routing commands for the framework: core plugin manifests
/// first, then catalog plugin manifests, each group sorted by plugin name
/// so the assembled command order is stable across restarts.
fn plugin_commands(
    core_manifests: &HashMap<String, Manifest>,
    catalog: &HashMap<String, CatalogEntry>,
) -> Vec<poise::Command<Data, Error>> {
    let mut commands = Vec::new();
    let mut core: Vec<&Manifest> = core_manifests.values().collect();
    core.sort_by(|a, b| a.name.cmp(&b.name));
    for manifest in core {
        commands.extend(commands_from_manifest(manifest));
    }
    let mut entries: Vec<&CatalogEntry> = catalog.values().collect();
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    for entry in entries {
        commands.extend(commands_from_manifest(&entry.manifest));
    }
    commands
}

/// Event handler for Discord gateway events.
pub struct BotEventHandler {
    event_bus: Arc<EventBus>,
    data: Arc<Data>,
    voice_subscriber: Arc<VoiceStateSubscriber>,
    http: Arc<poise::serenity_prelude::Http>,
    plugin_events: Arc<PluginEventRouter>,
}

impl BotEventHandler {
    pub fn new(
        event_bus: Arc<EventBus>,
        data: Arc<Data>,
        voice_subscriber: Arc<VoiceStateSubscriber>,
        http: Arc<poise::serenity_prelude::Http>,
        plugin_events: Arc<PluginEventRouter>,
    ) -> Self {
        Self {
            event_bus,
            data,
            voice_subscriber,
            http,
            plugin_events,
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

            let voice_states = {
                let Some(guild) = ctx.cache.guild(guild_id) else {
                    continue;
                };
                self.collect_voice_states_from_guild(&guild)
            };

            for (user_id, guild_id, channel_id, session_id) in voice_states {
                match self
                    .voice_subscriber
                    .track_existing_user(user_id, guild_id, channel_id, &session_id)
                    .await
                {
                    Ok(_) => tracked += 1,
                    Err(e) => {
                        error!("Failed to track existing user {user_id} in guild {guild_id}: {e}")
                    }
                }
            }
        }

        if tracked > 0 {
            info!("Voice channel scan complete: {tracked} users now being tracked");
        }
    }

    /// Collects voice state data from a guild reference.
    /// For large guilds the member list may be incomplete on `GuildCreate`; in that case
    /// we default to treating unknown users as non-bots (better to over-track than under-track).
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
                debug!("Bot version unchanged ({current_version})");
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

    /// Registers the auto-enabled plugins' commands in a guild in one bulk
    /// call unless the guild has explicitly disabled them. A missing
    /// `guild_plugins` row means enabled by default (auto-enable); an
    /// `enabled = false` row opts out. Failure to list state or register
    /// commands is logged and skipped — the bot stays up and the plugin's
    /// commands simply stay absent in that guild.
    async fn register_plugins_in_guild(&self, guild_id: poise::serenity_prelude::GuildId) {
        let rows = match self
            .data
            .repos
            .guild_plugins()
            .list_for_guild(guild_id.get())
            .await
        {
            Ok(rows) => rows,
            Err(e) => {
                warn!(
                    "failed to list guild plugins for guild {}: {e}",
                    guild_id.get()
                );
                return;
            }
        };

        // Collect every enabled auto-enabled plugin's commands first, then
        // register them in one bulk call: Discord's per-guild registration is
        // a bulk overwrite, so one call per plugin would clobber the others.
        let mut commands = Vec::new();
        for plugin_name in self.data.auto_enable_plugins() {
            let disabled = rows
                .iter()
                .any(|row| row.plugin_name == plugin_name && !row.enabled);
            if disabled {
                debug!(
                    "plugin `{plugin_name}` disabled in guild {}, skipping registration",
                    guild_id.get()
                );
                continue;
            }
            let Some(manifest) = self.data.manifest_for(&plugin_name) else {
                warn!(
                    "no manifest for auto-enabled plugin `{plugin_name}`; \
                     skipping registration in guild {}",
                    guild_id.get()
                );
                continue;
            };
            commands.extend(commands_from_manifest(manifest));
        }
        if commands.is_empty() {
            return;
        }
        if let Err(e) = register_in_guild(&self.http, &commands, guild_id).await {
            warn!(
                "failed to register auto-enabled plugin commands in guild {}: {e}",
                guild_id.get()
            );
        }
    }

    /// Routes a view interaction (component click or modal submit) to the
    /// plugin view session open for its message. The session's plugin
    /// renders a fresh spec; the interaction is acknowledged and the message
    /// body is replaced with the spec's raw data via a bare HTTP edit —
    /// `serenity::Component` is not `Deserialize`, so the spec cannot ride a
    /// typed `CreateReply`.
    ///
    /// An interaction without an open session is acknowledged and dropped
    /// (stale view); a dead plugin is acknowledged, logged, and its session
    /// is abandoned.
    async fn route_view_interaction(
        &self,
        message_id: MessageId,
        custom_id: &str,
        interaction: Value,
        channel_id: GenericChannelId,
        kind: &str,
    ) {
        let result = self
            .data
            .plugin_engine
            .interact(message_id, custom_id, interaction)
            .await;

        match result {
            Ok(spec) => {
                if let Err(e) = self
                    .http
                    .edit_message(channel_id, message_id, &spec.data, Vec::new())
                    .await
                {
                    warn!("failed to update message {message_id} after {kind}: {e}");
                }
            }
            Err(InteractionError::NoSession { .. }) => {
                debug!("{kind} on message {message_id} without an open session");
            }
            Err(InteractionError::Plugin(e)) => {
                warn!("plugin session for message {message_id} failed: {e}");
                if let Err(e) = self.data.plugin_engine.abandon(message_id).await {
                    warn!("failed to abandon session for message {message_id}: {e}");
                }
            }
            Err(e) => {
                warn!("{kind} on message {message_id} failed: {e}");
            }
        }
    }

    /// Routes a component interaction: acknowledges the click, then hands the
    /// interaction to the open view session.
    async fn handle_component_interaction(&self, interaction: &ComponentInteraction) {
        let message_id = interaction.message.id;

        // Acknowledge the click before the plugin round trip: Discord
        // requires a response within 3 seconds, and the interact call may
        // take most of that window. Best-effort — a failed ack is logged,
        // not fatal.
        if let Err(e) = interaction
            .create_response(&self.http, CreateInteractionResponse::Acknowledge)
            .await
        {
            warn!("failed to acknowledge component interaction on message {message_id}: {e}");
        }

        self.route_view_interaction(
            message_id,
            &interaction.data.custom_id,
            serde_json::to_value(interaction).unwrap_or_default(),
            interaction.channel_id,
            "component interaction",
        )
        .await;
    }

    /// Routes a modal submit like a component interaction: acknowledges the
    /// submit, then hands the interaction to the open view session for the
    /// message the modal was attached to.
    async fn handle_modal_submit_interaction(&self, interaction: &ModalInteraction) {
        if let Err(e) = interaction
            .create_response(&self.http, CreateInteractionResponse::Acknowledge)
            .await
        {
            warn!(
                "failed to acknowledge modal submit on message {}: {e}",
                interaction
                    .message
                    .as_ref()
                    .map(|m| m.id)
                    .unwrap_or_default()
            );
        }

        let Some(message) = interaction.message.as_ref() else {
            debug!("modal submit without a message; not routed");
            return;
        };

        self.route_view_interaction(
            message.id,
            &interaction.data.custom_id,
            serde_json::to_value(interaction).unwrap_or_default(),
            interaction.channel_id,
            "modal submit",
        )
        .await;
    }
}

#[async_trait]
impl poise::serenity_prelude::EventHandler for BotEventHandler {
    async fn dispatch(&self, ctx: &poise::serenity_prelude::Context, event: &FullEvent) {
        match event {
            FullEvent::Ready { .. } => {
                info!("Bot is ready, scanning voice channels...");
                self.scan_voice_channels(ctx).await;

                for guild_id in ctx.cache.guilds() {
                    self.register_plugins_in_guild(guild_id).await;
                }

                // Check if commands need to be re-registered
                self.register_commands_if_needed().await;
            }
            FullEvent::GuildCreate { guild, .. } => {
                self.register_plugins_in_guild(guild.id).await;

                let is_enabled = self
                    .data
                    .service
                    .voice_tracking
                    .is_enabled(guild.id.get())
                    .await;

                if !is_enabled {
                    return;
                }

                let voice_states = self.collect_voice_states_from_guild(guild);
                let mut tracked = 0u32;

                for (user_id, guild_id, channel_id, session_id) in voice_states {
                    match self
                        .voice_subscriber
                        .track_existing_user(user_id, guild_id, channel_id, &session_id)
                        .await
                    {
                        Ok(_) => tracked += 1,
                        Err(e) => error!(
                            "Failed to track existing user {user_id} in guild {guild_id}: {e}"
                        ),
                    }
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
                self.event_bus.publish(event.clone());
                self.plugin_events
                    .fan_out(&self.data.plugin_manager, VOICE_STATE_EVENT, &event)
                    .await;
            }
            FullEvent::InteractionCreate { interaction, .. } => match interaction {
                Interaction::Component(interaction) => {
                    self.handle_component_interaction(interaction).await;
                }
                Interaction::Modal(interaction) => {
                    self.handle_modal_submit_interaction(interaction).await;
                }
                _ => {}
            },
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::entry_named;
    use crate::test_helpers::manifest_named;

    #[test]
    fn plugin_commands_sort_each_group_by_plugin_name() {
        let core_manifests = HashMap::from([
            ("zeta".to_string(), manifest_named("zeta")),
            ("alpha".to_string(), manifest_named("alpha")),
        ]);
        let catalog = HashMap::from([
            ("mike".to_string(), entry_named("mike")),
            ("bravo".to_string(), entry_named("bravo")),
        ]);

        let commands = plugin_commands(&core_manifests, &catalog);
        let names: Vec<&str> = commands
            .iter()
            .map(|command| command.name.as_ref())
            .collect();

        assert_eq!(names, ["alpha", "zeta", "bravo", "mike"]);
    }
}
