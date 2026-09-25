//! Discord bot implementation and command handling.
//!
//! This module contains the main [`Bot`] struct which manages the Discord client,
//! and the [`BotEventHandler`] which processes gateway events. It acts as the
//! bridge between the Discord gateway and the application's internal services.

pub mod checks;
pub mod command;
pub mod error;
pub mod error_handler;
pub mod gui;
pub mod navigation;
pub mod reply;
pub mod translate;
pub mod utils;
pub mod view;

use std::collections::HashMap;
use std::collections::HashSet;
use std::future::Future;
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
use pwr_plugin_protocol::MODAL_OPENED_KIND;
use pwr_plugin_protocol::Manifest;
use pwr_plugin_protocol::VIEW_MOVED_KIND;
use pwr_plugin_protocol::ViewSpec;
use serde_json::Value;
use serde_json::json;

type Error = Box<dyn std::error::Error + Send + Sync>;

use crate::bot::command::Cog;
use crate::bot::command::Cogs;
use crate::bot::error_handler::ErrorHandler;
use crate::bot::translate::TranslateLayer;
use crate::config::Config;
use crate::entity::BotMetaKey;
use crate::event::event_bus::EventBus;
use crate::plugin::CatalogEntry;
use crate::plugin::GUILD_CREATE_EVENT;
use crate::plugin::HostConfig;
use crate::plugin::HostServices;
use crate::plugin::InstallError;
use crate::plugin::InteractionEngine;
use crate::plugin::InteractionError;
use crate::plugin::ModalDeliveryError;
use crate::plugin::PgKvStore;
use crate::plugin::PluginCatalog;
use crate::plugin::PluginEventRouter;
use crate::plugin::PluginManager;
use crate::plugin::RespawnPolicy;
use crate::plugin::RunningPlugin;
use crate::plugin::SerenityHostIo;
use crate::plugin::SerenityStatsSource;
use crate::plugin::SerenityUserResolver;
use crate::plugin::ServiceWelcomeSettingsSource;
use crate::plugin::StatsHandle;
use crate::plugin::UserResolverHandle;
use crate::plugin::VOICE_STATE_EVENT;
use crate::plugin::command::ACTOR_CONTEXT_KEY;
use crate::plugin::command::PluginRoutes;
use crate::plugin::command::actor_context_from_parts_with_guild;
use crate::plugin::command::commands_from_manifest;
use crate::plugin::command::register_in_guild;
use crate::plugin::command::routes_from_manifests_with_reserved;
use crate::plugin::decode_runtime_files_with_existing;
use crate::plugin::edit_body_for_transport;
use crate::plugin::interaction::DEFAULT_VIEW_TIMEOUT;
use crate::plugin::validate_view_spec;
use crate::repo::traits::Repos;
use crate::service::Services;
use crate::update::about::AboutStats;

/// Data shared across bot commands and contexts.
pub struct Data {
    pub config: Arc<Config>,
    pub service: Arc<Services>,
    pub repos: Arc<dyn Repos + Send + Sync>,
    pub plugin_manager: Arc<PluginManager>,
    pub plugin_catalog: Arc<HashMap<String, CatalogEntry>>,
    /// Why the plugin catalog failed to load, if it did: owners see the real
    /// path and cause through the `/plugins` commands while plain members
    /// keep the friendly empty-catalog message.
    pub plugin_catalog_error: Option<InstallError>,
    pub plugin_engine: Arc<InteractionEngine<RunningPlugin>>,
    /// Command-name → plugin-name routes for dispatch; built at startup from
    /// the loaded manifests ([`routes_from_manifests`]).
    pub plugin_routes: Arc<PluginRoutes>,
    /// Manifests of the core plugins spawned at startup, by plugin name.
    pub core_manifests: Arc<HashMap<String, Manifest>>,
    /// Tracks the messages owned by live Host (TEA) sessions, so the global
    /// event handler skips their interactions and the Host acknowledges them
    /// exactly once. See [`crate::bot::translate`].
    pub translate_layer: Arc<TranslateLayer>,
    /// The Settings section-handoff return waiters: the Router parks one per
    /// handed-off message, and the host-reserved `settings` open_view target
    /// completes it.
    pub settings_returns: Arc<translate::SettingsReturns>,
    /// Fills the attachment slots a plugin envelope declares at transport
    /// (ADR-0012).
    pub previews: Arc<crate::plugin::preview::PreviewResolver>,
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
        names.sort();
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
    pub http: Arc<Http>,
    client_builder: Option<ClientBuilder>,
    client: Arc<Mutex<Option<Client>>>,
    /// The real source behind the `host.stats` handle; its gateway cache is
    /// attached in [`Bot::start`] once the client is built.
    stats_source: Arc<SerenityStatsSource>,
    user_resolver: UserResolverHandle,
}

impl Bot {
    /// Creates a new bot instance with all required components.
    pub async fn new(
        config: Arc<Config>,
        event_bus: Arc<EventBus>,
        service: Arc<Services>,
        repos: Arc<dyn Repos + Send + Sync>,
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

        let (catalog, catalog_error) = Self::load_plugin_catalog(&config);
        let plugin_engine = Arc::new(InteractionEngine::<RunningPlugin>::with_timeout(
            DEFAULT_VIEW_TIMEOUT,
        ));
        let plugin_events = Arc::new(PluginEventRouter::new());
        // The stats handle is present from construction; the source riding it
        // is attached below once the command count is known, and the real
        // gateway cache attaches in `start()` after the client is built.
        let stats_handle = Arc::new(StatsHandle::default());
        let user_resolver = UserResolverHandle::default();
        // The welcome card renderer both the host ops and the view transports
        // share: one generator for the process (ADR-0012).
        let previews = Arc::new(crate::plugin::preview::PreviewResolver::new(vec![
            Arc::new(crate::plugin::preview::WelcomeAttachmentRenderer::new(
                service.settings.clone(),
                Arc::new(
                    crate::bot::command::welcome::image_generator::WelcomeImageGenerator::new(),
                ),
            )),
        ]));
        let settings_returns = Arc::new(translate::SettingsReturns::default());
        let host_services = Arc::new(HostServices {
            io: Some(Arc::new(SerenityHostIo::new(http.clone()))),
            config: Some(HostConfig::from(&*config)),
            kv: Some(Arc::new(PgKvStore::new(repos.plugin_kv()))),
            engine: Some(plugin_engine.clone()),
            stats: stats_handle.clone(),
            users: user_resolver.clone(),
            welcome: Some(Arc::new(ServiceWelcomeSettingsSource::new(
                service.settings.clone(),
            ))),
            previews: Some(previews.clone()),
            settings_returns: Some(settings_returns.clone()),
        });
        let plugin_manager = Arc::new(
            PluginManager::new(Some(http.clone()), RespawnPolicy::default())
                .with_host_services(host_services)
                .with_event_bus(event_bus.clone())
                .with_event_router(plugin_events.clone()),
        );

        // Core plugins are spawned once at startup. Their manifest event
        // handlers are subscribed after the handshake; per-guild command
        // registration follows in Ready/GuildCreate. A missing binary is not
        // fatal: the bot stays up and the plugin's commands stay unregistered.
        let mut manifest_sources: Vec<(String, Option<Manifest>)> = Vec::new();
        for spec in &config.core_plugins {
            match plugin_manager
                .spawn(&spec.name, &spec.path, None, &[], &[])
                .await
            {
                Ok(plugin) => {
                    if let Some(manifest) = plugin.manifest() {
                        plugin_events
                            .subscribe(&spec.name, &manifest.event_handlers)
                            .await;
                    }
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
        route_sources.sort_by(|(left_name, _), (right_name, _)| {
            let left_is_catalog = !core_manifests.contains_key(left_name);
            let right_is_catalog = !core_manifests.contains_key(right_name);
            right_is_catalog
                .cmp(&left_is_catalog)
                .then_with(|| left_name.cmp(right_name))
        });
        let host_command_names = host_command_names();
        let plugin_routes = Arc::new(routes_from_manifests_with_reserved(
            route_sources,
            &host_command_names,
        ));

        let framework =
            Self::create_framework(&config, &catalog, &core_manifests, &host_command_names)?;

        let start_time = Instant::now();
        let data = Arc::new(Data {
            config: config.clone(),
            service,
            repos,
            plugin_manager,
            plugin_catalog: Arc::new(catalog),
            plugin_catalog_error: catalog_error,
            plugin_engine,
            plugin_routes,
            core_manifests,
            translate_layer: Arc::new(TranslateLayer::new()),
            settings_returns,
            previews: previews.clone(),
            start_time,
        });

        // The command count comes from the finished framework options, so the
        // source can only ride the pre-start handle here — after the core
        // plugins spawned. Calls before this point answer `HostUnavailable`
        // (the handle serves a typed error until a source is attached).
        let stats_source = Arc::new(SerenityStatsSource::new(
            http.clone(),
            start_time,
            config.version.clone(),
            AboutStats::count_commands(&framework.options().commands),
        ));
        stats_handle.attach(stats_source.clone());

        let event_handler = Arc::new(BotEventHandler::new(
            data.clone(),
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
            http,
            client_builder: Some(client_builder),
            client: Arc::new(Mutex::new(None)),
            stats_source,
            user_resolver,
        })
    }

    /// Starts the bot client in a background task.
    pub fn start(&mut self) {
        info!("Starting bot client...");
        let client_builder = self.client_builder.take().expect("start() called twice");
        let client = self.client.clone();
        let stats_source = self.stats_source.clone();
        let user_resolver = self.user_resolver.clone();
        let http = self.http.clone();

        tokio::spawn(async move {
            info!("Connecting bot to Discord...");

            let built_client = client_builder
                .await
                .expect("Failed to build Discord client");

            // The gateway cache only exists after the build; from here the
            // `host.stats` gather serves live guild/user counts.
            stats_source.attach_cache(built_client.cache.clone());
            user_resolver.attach(Arc::new(SerenityUserResolver::new(
                built_client.cache.clone(),
                http,
            )));

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
    /// routing command per catalog plugin manifest not itself a core plugin
    /// — each group sorted by plugin name for a stable order across restarts
    /// (see [`plugin_commands`]) — so plugin commands are registered on the
    /// framework before `Framework::builder().build()`.
    fn create_framework(
        config: &Config,
        catalog: &HashMap<String, CatalogEntry>,
        core_manifests: &HashMap<String, Manifest>,
        host_command_names: &HashSet<String>,
    ) -> Result<Box<Framework<Data, Error>>> {
        let mut commands = Cogs.commands();
        commands.extend(plugin_commands(core_manifests, catalog, host_command_names));

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
    /// catalog keeps the bot up with plugin commands absent rather than
    /// failing startup, but the failure is logged loudly and returned so
    /// owners see the real path and cause through the `/plugins`
    /// commands ([`Data::plugin_catalog_error`]).
    fn load_plugin_catalog(
        config: &Config,
    ) -> (HashMap<String, CatalogEntry>, Option<InstallError>) {
        match PluginCatalog::load(&config.plugins_toml) {
            Ok(catalog) => (catalog, None),
            Err(e) => {
                error!(
                    "failed to load plugin catalog `{}`: {e}",
                    config.plugins_toml.display()
                );
                (HashMap::new(), Some(e))
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

pub(crate) fn host_command_names() -> HashSet<String> {
    Cogs.commands()
        .into_iter()
        .map(|command| command.name.to_string())
        .collect()
}

/// The plugin routing commands for the framework: core plugin manifests
/// first, then catalog plugin manifests, each group sorted by plugin name
/// so the assembled command order is stable across restarts.
///
/// Host Cog roots and earlier plugin roots are reserved. A catalog entry
/// whose plugin also runs as a core plugin contributes nothing: its commands
/// come from the core manifest only. Catalog entries for core plugins exist so
/// `/plugins list` and the install/update sources see them.
fn plugin_commands(
    core_manifests: &HashMap<String, Manifest>,
    catalog: &HashMap<String, CatalogEntry>,
    reserved_names: &HashSet<String>,
) -> Vec<poise::Command<Data, Error>> {
    let mut commands = Vec::new();
    let mut registered_names = HashSet::new();
    let mut core: Vec<&Manifest> = core_manifests.values().collect();
    core.sort_by(|a, b| a.name.cmp(&b.name));
    for manifest in core {
        add_plugin_commands(
            &mut commands,
            &mut registered_names,
            manifest,
            reserved_names,
        );
    }
    let mut entries: Vec<&CatalogEntry> = catalog.values().collect();
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    for entry in entries {
        if core_manifests.contains_key(&entry.name) {
            warn!(
                "skipping catalog commands for `{}`: it is a core plugin, its commands come from the core manifest",
                entry.name
            );
            continue;
        }
        add_plugin_commands(
            &mut commands,
            &mut registered_names,
            &entry.manifest,
            reserved_names,
        );
    }
    commands
}

pub(crate) fn add_plugin_commands(
    commands: &mut Vec<poise::Command<Data, Error>>,
    registered_names: &mut HashSet<String>,
    manifest: &Manifest,
    reserved_names: &HashSet<String>,
) {
    for command in commands_from_manifest(manifest) {
        let name = command.name.as_ref().to_string();
        if reserved_names.contains(&name) {
            warn!("skipping plugin command `{name}`: a host Cog owns that path");
            continue;
        }
        if !registered_names.insert(name.clone()) {
            warn!("skipping duplicate plugin command `{name}`: the first owner wins");
            continue;
        }
        commands.push(command);
    }
}

/// What a plugin's answer to a view interaction means for the message it
/// edits. See [`BotEventHandler::view_answer`].
enum ViewAnswer {
    /// The fresh render to send: the edit body, with its declared attachment
    /// slots resolved, plus the files that declaration names (ADR-0012).
    Render(Value, Vec<CreateAttachment<'static>>),
    /// The plugin answered the interaction itself — it opened a modal as the
    /// click's response (ADR-0011). The host sends nothing.
    Answered,
    /// Nothing to send: a stale view or a failed plugin session.
    Nothing,
}

/// How long a plugin round trip may take before the host acknowledges the
/// click to stay inside Discord's response window. Comfortably under the
/// 3-second limit: the window covers the host's own HTTP round trip too.
const ACK_FALLBACK_WINDOW: Duration = Duration::from_millis(2500);

/// Races a plugin round trip against the click's response window: the
/// trip's result when it beats the window, `None` when the window elapses
/// first. The trip is borrowed, never cancelled — a slow trip keeps running
/// (it holds the session lock) and the caller awaits it after the fallback.
async fn within_ack_window<F: Future + Unpin>(
    round_trip: &mut F,
    window: Duration,
) -> Option<F::Output> {
    tokio::select! {
        biased;
        result = round_trip => Some(result),
        () = tokio::time::sleep(window) => None,
    }
}

/// Event handler for Discord gateway events.
pub struct BotEventHandler {
    data: Arc<Data>,
    http: Arc<poise::serenity_prelude::Http>,
    plugin_events: Arc<PluginEventRouter>,
}

impl BotEventHandler {
    pub fn new(
        data: Arc<Data>,
        http: Arc<poise::serenity_prelude::Http>,
        plugin_events: Arc<PluginEventRouter>,
    ) -> Self {
        Self {
            data,
            http,
            plugin_events,
        }
    }

    async fn fan_out_guild_create(&self, payload: Value) {
        self.plugin_events
            .fan_out(&self.data.plugin_manager, GUILD_CREATE_EVENT, &payload)
            .await;
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

                let mut commands = Cogs.commands();
                commands.extend(plugin_commands(
                    &self.data.core_manifests,
                    &self.data.plugin_catalog,
                    &host_command_names(),
                ));
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
    /// commands is logged at error level and skipped — the bot stays up and
    /// the plugin's commands simply stay absent in that guild.
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
                error!(
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
        let mut registered_names = HashSet::new();
        let host_command_names = host_command_names();
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
                let catalog_suffix = match self.data.plugin_catalog_error.as_ref() {
                    Some(catalog_error) => {
                        format!(": the plugin catalog failed to load: {catalog_error}")
                    }
                    None => String::new(),
                };
                error!(
                    "no manifest for auto-enabled plugin `{plugin_name}`; \
                     skipping registration in guild {}{catalog_suffix}",
                    guild_id.get()
                );
                continue;
            };
            add_plugin_commands(
                &mut commands,
                &mut registered_names,
                manifest,
                &host_command_names,
            );
        }
        if commands.is_empty() {
            return;
        }
        if let Err(e) = register_in_guild(&self.http, &commands, guild_id).await {
            error!(
                "failed to register auto-enabled plugin commands in guild {}: {e}",
                guild_id.get()
            );
        }
    }

    /// Routes a view interaction (component click or modal submit) to the
    /// plugin view session open for its message. The session's plugin
    /// renders a fresh spec; the interaction is acknowledged and the
    /// message body is replaced with the spec's raw data through the
    /// interaction webhook — `serenity::Component` is not `Deserialize`, so
    /// the spec cannot ride a typed `CreateReply`.
    ///
    /// The webhook route (`@original`) is the only edit route that works
    /// for both ephemeral and public responses; the channel-message route
    /// rejects ephemeral messages with `Missing Access`. The `Acknowledge`
    /// response pins `@original` to the message the interaction was fired
    /// on.
    ///
    /// Host-owned messages never reach here: the handlers skip them before
    /// acknowledging, so a "no open session" result below is always a
    /// genuinely stale plugin view.
    ///
    /// An interaction without an open session is acknowledged and dropped
    /// (stale view); a dead plugin is acknowledged, logged, and its session
    /// is abandoned.
    async fn route_view_interaction(
        &self,
        message_id: MessageId,
        custom_id: &str,
        interaction: Value,
        interaction_token: &str,
        guild_id: Option<u64>,
        kind: &str,
    ) {
        let result = self
            .data
            .plugin_engine
            .interact_validated(message_id, custom_id, interaction, validate_view_spec)
            .await;

        if let ViewAnswer::Render(body, files) =
            self.view_answer(result, message_id, guild_id, kind).await
            && let Err(e) = self
                .http
                .edit_original_interaction_response(
                    interaction_token,
                    &body,
                    files.into_iter().map(Into::into).collect(),
                )
                .await
        {
            warn!("failed to update message {message_id} after {kind}: {e}");
        }
    }

    /// Decides what a plugin's answer to a view interaction means for the
    /// message: the fresh render to send (its declared attachment slots
    /// filled, ADR-0012), nothing because the plugin answered the
    /// interaction itself, or nothing because the view is stale or the
    /// session failed.
    async fn view_answer(
        &self,
        result: Result<ViewSpec, InteractionError>,
        message_id: MessageId,
        guild_id: Option<u64>,
        kind: &str,
    ) -> ViewAnswer {
        match result {
            // The plugin's payload is a create envelope: the edit transport
            // strips the create-only fields Discord rejects on edit (error
            // 50080 for `sticker_ids`) before the body is sent.
            Ok(spec) => {
                let (body, mut files) = self
                    .data
                    .previews
                    .resolve(edit_body_for_transport(&spec.data), guild_id)
                    .await;
                match decode_runtime_files_with_existing(&spec.files, files.len()) {
                    Ok(runtime_files) => files.extend(runtime_files),
                    Err(error) => {
                        warn!("plugin returned invalid runtime files: {}", error.msg);
                        return ViewAnswer::Nothing;
                    }
                }
                ViewAnswer::Render(body, files)
            }
            // A modal trigger the plugin answered by opening the modal: the
            // modal IS the response, so the host sends nothing and the
            // panel stays as it was.
            Err(InteractionError::PluginRejected { kind, .. }) if kind == MODAL_OPENED_KIND => {
                debug!("plugin answered the interaction with a modal of its own");
                ViewAnswer::Answered
            }
            // A nav click the plugin answered with an in-place open_view: the
            // click's message now shows the opened panel, so the host sends
            // nothing — its own render would overwrite the panel.
            Err(InteractionError::PluginRejected { kind, .. }) if kind == VIEW_MOVED_KIND => {
                debug!("plugin moved the message to the opened panel's view");
                ViewAnswer::Nothing
            }
            Err(InteractionError::NoSession { .. }) => {
                debug!("{kind} on message {message_id} without an open session");
                ViewAnswer::Nothing
            }
            Err(InteractionError::Plugin(e)) => {
                warn!("plugin session for message {message_id} failed: {e}");
                if let Err(e) = self.data.plugin_engine.abandon(message_id).await {
                    warn!("failed to abandon session for message {message_id}: {e}");
                }
                ViewAnswer::Nothing
            }
            Err(e) => {
                warn!("{kind} on message {message_id} failed: {e}");
                ViewAnswer::Nothing
            }
        }
    }

    /// Routes a component interaction: the plugin's fresh render answers the
    /// click directly when the round trip beats Discord's response window,
    /// and an acknowledge-plus-webhook-edit carries it when it does not.
    ///
    /// Messages owned by a live Host session are skipped — the Host loop
    /// owns both the routing and the acknowledgement there, so acking here
    /// would answer the interaction twice.
    async fn handle_component_interaction(&self, interaction: &ComponentInteraction) {
        let message_id = interaction.message.id;

        if self.data.translate_layer.host_owned(message_id) {
            return;
        }

        let guild_id = interaction.guild_id.map(GuildId::get);
        let mut raw = serde_json::to_value(interaction).unwrap_or_default();
        if let Some(raw_object) = raw.as_object_mut() {
            raw_object.insert(
                ACTOR_CONTEXT_KEY.into(),
                actor_context_from_parts_with_guild(
                    interaction.user.id,
                    interaction.member.as_deref(),
                    interaction.guild_id,
                    None,
                ),
            );
        }
        let round_trip = self.data.plugin_engine.interact_validated(
            message_id,
            &interaction.data.custom_id,
            raw,
            validate_view_spec,
        );
        tokio::pin!(round_trip);

        // Discord wants a response within 3 seconds. A round trip that beats
        // the window answers with its render, so the panel updates in one
        // step and a plugin that opened a modal itself (ADR-0011) is left to
        // own the response. A slow trip acknowledges first; the round trip is
        // polled, never cancelled — it holds the session lock — and its
        // render follows through the interaction webhook.
        let fast = within_ack_window(&mut round_trip, ACK_FALLBACK_WINDOW).await;

        match fast {
            Some(result) => match self
                .view_answer(result, message_id, guild_id, "component interaction")
                .await
            {
                ViewAnswer::Render(body, files) => {
                    let response = json!({ "type": 7, "data": body });
                    if let Err(e) = self
                        .http
                        .create_interaction_response(
                            interaction.id,
                            interaction.token.as_str(),
                            &response,
                            files.iter().cloned().map(Into::into).collect(),
                        )
                        .await
                    {
                        // The interaction was already answered by the plugin
                        // (a modal opened mid-round-trip): fall back to the
                        // webhook edit rather than leave a stale panel.
                        debug!("failed to answer component interaction on {message_id}: {e}");
                        self.edit_after_ack(message_id, interaction.token.as_str(), body, files)
                            .await;
                    }
                }
                ViewAnswer::Answered => {}
                ViewAnswer::Nothing => self.ack_component(interaction, message_id).await,
            },
            None => {
                self.ack_component(interaction, message_id).await;
                let result = (&mut round_trip).await;
                if let ViewAnswer::Render(body, files) = self
                    .view_answer(result, message_id, guild_id, "component interaction")
                    .await
                {
                    self.edit_after_ack(message_id, interaction.token.as_str(), body, files)
                        .await;
                }
            }
        }
    }

    /// Acknowledges a click without a visible reply, leaving the message for
    /// a later webhook edit.
    async fn ack_component(&self, interaction: &ComponentInteraction, message_id: MessageId) {
        if let Err(e) = interaction
            .create_response(&self.http, CreateInteractionResponse::Acknowledge)
            .await
        {
            warn!("failed to acknowledge component interaction on message {message_id}: {e}");
        }
    }

    /// Carries a render to a message whose interaction was already
    /// acknowledged: the body rides the interaction webhook, the only edit
    /// route that works for ephemeral responses too.
    async fn edit_after_ack(
        &self,
        message_id: MessageId,
        token: &str,
        body: Value,
        files: Vec<CreateAttachment<'static>>,
    ) {
        if let Err(e) = self
            .http
            .edit_original_interaction_response(
                token,
                &body,
                files.into_iter().map(Into::into).collect(),
            )
            .await
        {
            warn!("failed to update message {message_id} after a component interaction: {e}");
        }
    }

    /// Routes a modal submit: a submission of a plugin-opened modal rides
    /// the author route to the session that opened it, and that session's
    /// answer renders as the submission's response — the interaction stays
    /// unanswered until then, so the answer is the first response (the
    /// plugin-session analogue of the host's modal flow, ADR-0007). A
    /// consumed route bypasses the message-keyed route entirely: one
    /// submission, one session.
    ///
    /// Everything except a terminal invalid owner response routes like a
    /// component interaction: acknowledge the submit, then hand the
    /// interaction to the open view session for the message the modal was
    /// attached to. An invalid owner response returns without a second
    /// acknowledgement or commit.
    ///
    /// Modal submissions on Host-owned messages are skipped — the poise
    /// modal task the Host's feature spawned acknowledges the submission
    /// itself, so acking here would answer it twice.
    async fn handle_modal_submit_interaction(&self, interaction: &ModalInteraction) {
        let attached_message = interaction.message.as_ref();

        if attached_message.is_some_and(|message| self.data.translate_layer.host_owned(message.id))
        {
            return;
        }

        // Plugin-opened modal: deliver first, so the route consumption
        // decides who answers. A terminal invalid response must not fall
        // through: the message-keyed route would call the plugin a second
        // time and could commit a view that was already rejected.
        let raw = serde_json::to_value(interaction).unwrap_or_default();
        match self
            .data
            .plugin_manager
            .deliver_modal_submission(interaction.user.id.get(), raw)
            .await
        {
            Ok(spec) => {
                self.render_modal_submission_response(interaction, &spec)
                    .await;
                return;
            }
            Err(ModalDeliveryError::InvalidResponse { kind, msg }) => {
                warn!("plugin returned an invalid modal response ({kind}): {msg}");
                return;
            }
            Err(error) => {
                debug!("modal submit not owned by a plugin session: {error}");
            }
        }

        if let Err(e) = interaction
            .create_response(&self.http, CreateInteractionResponse::Acknowledge)
            .await
        {
            warn!(
                "failed to acknowledge modal submit on message {}: {e}",
                attached_message.map(|m| m.id).unwrap_or_default()
            );
        }

        let Some(message) = attached_message else {
            debug!("modal submit without a message; not routed");
            return;
        };

        self.route_view_interaction(
            message.id,
            &interaction.data.custom_id,
            serde_json::to_value(interaction).unwrap_or_default(),
            interaction.token.as_str(),
            interaction.guild_id.map(GuildId::get),
            "modal submit",
        )
        .await;
    }

    /// Renders a plugin's answer to its own modal submission as the
    /// submission's response: an update-message carrying the answer's
    /// create envelope through the edit-transport strip (the same
    /// projection every edit sends) with its declared attachment slots
    /// filled (ADR-0012). A submission opened from a component click replaces
    /// that message; one opened from a command answers as a fresh message,
    /// honoring the spec's ephemerality.
    async fn render_modal_submission_response(
        &self,
        interaction: &ModalInteraction,
        spec: &ViewSpec,
    ) {
        let kind = if interaction.message.is_some() { 7 } else { 4 };
        let (mut data, mut files) = self
            .data
            .previews
            .resolve(
                edit_body_for_transport(&spec.data),
                interaction.guild_id.map(GuildId::get),
            )
            .await;
        match decode_runtime_files_with_existing(&spec.files, files.len()) {
            Ok(runtime_files) => files.extend(runtime_files),
            Err(error) => {
                warn!("plugin returned invalid runtime files: {}", error.msg);
                return;
            }
        }
        if kind == 4 && spec.ephemeral {
            let flags = data
                .get("flags")
                .and_then(Value::as_u64)
                .unwrap_or(u64::from(MessageFlags::IS_COMPONENTS_V2.bits()));
            data["flags"] = Value::from(flags | u64::from(MessageFlags::EPHEMERAL.bits()));
        }
        let body = json!({ "type": kind, "data": data });
        if let Err(e) = self
            .http
            .create_interaction_response(
                interaction.id,
                interaction.token.as_str(),
                &body,
                files.into_iter().map(Into::into).collect(),
            )
            .await
        {
            warn!("failed to answer modal submit {}: {e}", interaction.id);
        }
    }
}

#[async_trait]
impl poise::serenity_prelude::EventHandler for BotEventHandler {
    async fn dispatch(&self, ctx: &poise::serenity_prelude::Context, event: &FullEvent) {
        match event {
            FullEvent::Ready { .. } => {
                info!("Bot is ready");
                for guild_id in ctx.cache.guilds() {
                    let payload = ctx
                        .cache
                        .guild(guild_id)
                        .map(|guild| json!({ "guild": &*guild }));
                    if let Some(payload) = payload {
                        self.fan_out_guild_create(payload).await;
                    }
                    self.register_plugins_in_guild(guild_id).await;
                }
                self.register_commands_if_needed().await;
            }
            FullEvent::GuildCreate { guild, .. } => {
                self.register_plugins_in_guild(guild.id).await;
                let payload = json!({ "guild": guild });
                self.fan_out_guild_create(payload).await;
            }
            FullEvent::VoiceStateUpdate { old, new, .. } => {
                let payload = json!({ "old": old, "new": new });
                self.plugin_events
                    .fan_out(&self.data.plugin_manager, VOICE_STATE_EVENT, &payload)
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
    use std::fs;
    use std::path::PathBuf;
    use std::sync::Arc;

    use httpmock::Method;
    use httpmock::MockServer;
    use poise::serenity_prelude::Http;
    use tempfile::tempdir;

    use super::*;
    use crate::repo::PgRepos;
    use crate::service::Services;
    use crate::test_helpers::entry_named;
    use crate::test_helpers::manifest_named;

    #[tokio::test]
    async fn invalid_modal_response_does_not_fall_through_to_view_interaction() {
        let server = MockServer::start();
        let post = server.mock(|when, then| {
            when.method(Method::POST);
            then.status(200);
        });
        let patch = server.mock(|when, then| {
            when.method(Method::PATCH);
            then.status(200);
        });

        let manager = Arc::new(PluginManager::new(None, RespawnPolicy::default()));
        let plugin = manager
            .spawn(
                "malformed-modal",
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/fixtures/malformed_modal_plugin.sh"),
                None,
                &[],
                &[],
            )
            .await
            .expect("spawn malformed modal fixture");
        let engine: Arc<InteractionEngine<RunningPlugin>> = Arc::new(InteractionEngine::new());
        let message_id = MessageId::new(42);
        let user_id = UserId::new(7);
        let prior = ViewSpec {
            data: json!({"content": "prior"}),
            ephemeral: false,
            view: json!({"page": 1}),
            files: vec![],
        };
        engine
            .register(message_id, user_id, plugin.clone(), "modal", prior.clone())
            .await;
        manager
            .bind_modal(user_id.get(), "malformed-modal", "modal:submit")
            .await;

        let repos: Arc<dyn Repos + Send + Sync> = Arc::new(
            PgRepos::new("postgres://test.invalid/pwr_bot")
                .await
                .expect("construct test repositories"),
        );
        let data = Arc::new(Data {
            config: Arc::new(Config::default()),
            service: Arc::new(
                Services::new(repos.clone())
                    .await
                    .expect("construct services"),
            ),
            repos,
            plugin_manager: manager,
            plugin_catalog: Arc::new(HashMap::new()),
            plugin_catalog_error: None,
            plugin_engine: engine.clone(),
            plugin_routes: Arc::new(HashMap::new()),
            core_manifests: Arc::new(HashMap::new()),
            translate_layer: Arc::new(TranslateLayer::new()),
            settings_returns: Arc::new(crate::bot::translate::SettingsReturns::default()),
            previews: Arc::new(crate::plugin::preview::PreviewResolver::new(vec![])),
            start_time: Instant::now(),
        });
        let mut http = Http::without_token();
        http.ratelimiter = None;
        http.proxy = Some(server.url("").parse().expect("test proxy address"));
        let handler = BotEventHandler::new(
            data,
            Arc::new(http),
            Arc::new(crate::plugin::PluginEventRouter::new()),
        );

        let mut interaction: ModalInteraction = serde_json::from_str(
            &serde_json::to_string(&json!({
                "id": "1",
                "application_id": "1",
                "data": {"custom_id": "modal:submit", "components": []},
                "channel": {"id": "9", "type": 0},
                "channel_id": "9",
                "user": {"id": "7", "username": "tester"},
                "token": "token",
                "version": 1,
                "app_permissions": "0",
                "locale": "en-US",
                "entitlements": [],
                "attachment_size_limit": 1024
            }))
            .expect("serialize modal interaction"),
        )
        .expect("construct modal interaction");
        let mut message = Message::default();
        message.id = message_id;
        message.channel_id = ChannelId::new(9).into();
        interaction.message = Some(Box::new(message));

        handler.handle_modal_submit_interaction(&interaction).await;

        assert_eq!(engine.view_state(message_id).await, Some(prior.view));
        post.assert_hits(0);
        patch.assert_hits(0);
        plugin.stop().await.expect("stop malformed modal fixture");
    }

    /// A `Config` whose catalog path points at `path`, nothing else set.
    fn config_with_catalog(path: PathBuf) -> Config {
        Config {
            plugins_toml: path,
            ..Default::default()
        }
    }

    /// A one-entry `plugins.toml` for a plugin named `name`.
    fn catalog_text(name: &str) -> String {
        let manifest = serde_json::to_string(&manifest_named(name)).unwrap();
        format!(
            "[[plugins]]\nname = \"{name}\"\nurl = \"https://example.com/{name}\"\n\
             sha256 = \"{}\"\nmanifest = '{manifest}'\n",
            "ab".repeat(32)
        )
    }

    #[test]
    fn load_plugin_catalog_surfaces_a_missing_file_as_an_error() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("plugins.toml");

        let (catalog, error) = Bot::load_plugin_catalog(&config_with_catalog(path.clone()));

        assert!(catalog.is_empty());
        let error = error.expect("a missing catalog must surface its load error");
        match error {
            InstallError::Catalog {
                path: error_path, ..
            } => assert_eq!(error_path, path),
            other => panic!("expected a catalog error, got {other:?}"),
        }
    }

    #[test]
    fn load_plugin_catalog_surfaces_an_invalid_entry_as_an_error() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("plugins.toml");
        fs::write(
            &path,
            catalog_text("hello").replace("https://example.com/hello", "http://example.com/hello"),
        )
        .unwrap();

        let (catalog, error) = Bot::load_plugin_catalog(&config_with_catalog(path));

        assert!(catalog.is_empty());
        let error = error.expect("an invalid entry must surface its load error");
        let detail = error.to_string();
        assert!(
            detail.contains("must be https"),
            "the error carries the real validation cause: {detail}"
        );
    }

    #[test]
    fn load_plugin_catalog_reports_no_error_for_a_valid_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("plugins.toml");
        fs::write(&path, catalog_text("hello")).unwrap();

        let (catalog, error) = Bot::load_plugin_catalog(&config_with_catalog(path));

        assert!(error.is_none(), "a valid catalog must not report an error");
        assert_eq!(
            catalog.keys().map(String::as_str).collect::<Vec<_>>(),
            ["hello"]
        );
    }

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

        let commands = plugin_commands(&core_manifests, &catalog, &HashSet::new());
        let names: Vec<&str> = commands
            .iter()
            .map(|command| command.name.as_ref())
            .collect();

        assert_eq!(names, ["alpha", "zeta", "bravo", "mike"]);
    }

    #[test]
    fn host_cog_command_names_win_over_plugin_routes_and_registration() {
        let mut manifest = manifest_named("plugin");
        manifest.commands = vec![pwr_plugin_protocol::CommandDef {
            create_command: serde_json::json!({
                "name": "settings",
                "description": "Plugin settings"
            }),
        }];
        let core_manifests = HashMap::from([("plugin".to_string(), manifest.clone())]);
        let reserved = host_command_names();
        let routes = routes_from_manifests_with_reserved(
            [("plugin".to_string(), Some(manifest))],
            &reserved,
        );
        let commands = plugin_commands(&core_manifests, &HashMap::new(), &reserved);

        assert!(!routes.contains_key("settings"));
        assert!(
            commands
                .iter()
                .all(|command| command.name.as_ref() != "settings")
        );
    }

    /// A catalog entry for a plugin that also runs as a core plugin is
    /// skipped: the core manifest is the only source for its commands, so
    /// `set_commands` never sees the same command name twice.
    #[test]
    fn plugin_commands_skip_a_catalog_entry_that_duplicates_a_core_plugin() {
        let core_manifests = HashMap::from([("settings".to_string(), manifest_named("settings"))]);
        let catalog = HashMap::from([("settings".to_string(), entry_named("settings"))]);

        let commands = plugin_commands(&core_manifests, &catalog, &HashSet::new());
        let names: Vec<&str> = commands
            .iter()
            .map(|command| command.name.as_ref())
            .collect();

        assert_eq!(names, ["settings"]);
    }
    /// A catalog entry whose plugin is not a core plugin still contributes
    /// its commands.
    #[test]
    fn plugin_commands_keep_a_catalog_entry_whose_plugin_is_not_core() {
        let core_manifests = HashMap::from([("settings".to_string(), manifest_named("settings"))]);
        let catalog = HashMap::from([("greet".to_string(), entry_named("greet"))]);

        let commands = plugin_commands(&core_manifests, &catalog, &HashSet::new());
        let names: Vec<&str> = commands
            .iter()
            .map(|command| command.name.as_ref())
            .collect();

        assert_eq!(names, ["settings", "greet"]);
    }

    /// The full registered command surface — the host Cog commands plus one
    /// routing command per core plugin manifest, assembled through the same
    /// `commands_from_manifest` path the host registers through — pinned
    /// against a committed snapshot. With `UPDATE_SNAPSHOT=1` the test
    /// rewrites the fixture instead of asserting.
    #[test]
    fn the_registered_command_surface_matches_the_committed_snapshot() {
        let manifest_fixture = fs::read_to_string(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/core_manifests.json"),
        )
        .expect("core manifest fixture is readable");
        let manifests: Vec<Manifest> =
            serde_json::from_str(&manifest_fixture).expect("core manifest fixture is valid");
        let core_manifests = manifests
            .into_iter()
            .map(|manifest| (manifest.name.clone(), manifest))
            .collect();
        let mut commands = Cogs.commands();
        commands.extend(plugin_commands(
            &core_manifests,
            &HashMap::new(),
            &HashSet::new(),
        ));

        let mut surface =
            serde_json::to_value(poise::builtins::create_application_commands(&commands))
                .expect("the application commands serialize");
        let serde_json::Value::Array(entries) = &mut surface else {
            panic!("create_application_commands returns a list");
        };
        entries.sort_by_key(|entry| entry["name"].as_str().unwrap_or("").to_string());
        let snapshot = serde_json::to_string_pretty(&surface).expect("snapshot serializes") + "\n";

        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/registered_commands.json");
        if std::env::var_os("UPDATE_SNAPSHOT").is_some() {
            fs::write(&path, &snapshot).expect("snapshot fixture is writable");
            return;
        }

        let committed = fs::read_to_string(&path).unwrap_or_else(|error| {
            panic!(
                "the command snapshot is missing at {}: run \
                     `UPDATE_SNAPSHOT=1 cargo test the_registered_command_surface` to \
                     regenerate it ({error})",
                path.display()
            )
        });
        assert_eq!(
            snapshot, committed,
            "the registered command surface drifted from \
             tests/fixtures/registered_commands.json — if the change is \
             deliberate, run `UPDATE_SNAPSHOT=1 cargo test \
             the_registered_command_surface` and commit the fixture"
        );
    }

    /// A round trip that beats the window resolves to its result.
    #[tokio::test]
    async fn a_fast_round_trip_beats_the_ack_window() {
        let mut trip = Box::pin(async { "fast" });
        assert_eq!(
            within_ack_window(&mut trip, Duration::from_millis(50)).await,
            Some("fast")
        );
    }

    /// A slow round trip loses the window: the host learns it must ack.
    #[tokio::test]
    async fn a_slow_round_trip_loses_the_ack_window() {
        let mut trip = Box::pin(async {
            tokio::time::sleep(Duration::from_secs(60)).await;
            "slow"
        });
        assert_eq!(
            within_ack_window(&mut trip, Duration::from_millis(10)).await,
            None
        );
    }

    /// Losing the window does not cancel the round trip — it holds the
    /// session lock, so abandoning it would deadlock every later click on
    /// that panel. The still-running trip completes afterwards and its
    /// result is collected by awaiting it again.
    #[tokio::test]
    async fn a_timed_out_round_trip_is_not_cancelled() {
        let mut trip = Box::pin(async {
            tokio::time::sleep(Duration::from_millis(30)).await;
            "late"
        });
        assert_eq!(
            within_ack_window(&mut trip, Duration::from_millis(10)).await,
            None
        );
        assert_eq!((&mut trip).await, "late");
    }
}
