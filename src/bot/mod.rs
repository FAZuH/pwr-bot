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
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Duration;
use std::time::Instant;

use anyhow;
use anyhow::Result;
use arc_swap::ArcSwap;
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
use crate::bot::error::BotError;
use crate::bot::error_handler::ErrorHandler;
use crate::bot::translate::TranslateLayer;
use crate::config::Config;
use crate::entity::BotMetaKey;
use crate::event::event_bus::EventBus;
use crate::plugin::AuthoritySnapshot;
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
use crate::plugin::PluginError;
use crate::plugin::PluginEventRouter;
use crate::plugin::PluginManager;
use crate::plugin::ReloadPlan;
use crate::plugin::RespawnPolicy;
use crate::plugin::RunningPlugin;
use crate::plugin::SerenityHostIo;
use crate::plugin::SerenityStatsSource;
use crate::plugin::SerenityUserResolver;
use crate::plugin::StatsHandle;
use crate::plugin::UserResolverHandle;
use crate::plugin::VOICE_STATE_EVENT;
use crate::plugin::catalog_writer::check_grant;
use crate::plugin::command::ACTOR_CONTEXT_KEY;
use crate::plugin::command::PluginRoutes;
use crate::plugin::command::actor_context_from_parts_with_guild;
use crate::plugin::command::commands_from_manifest;
use crate::plugin::command::register_in_guild;
use crate::plugin::command::routes_from_manifests_with_reserved;
use crate::plugin::decode_runtime_files;
use crate::plugin::edit_body_for_transport;
use crate::plugin::install::install_verified;
use crate::plugin::interaction::DEFAULT_VIEW_TIMEOUT;
use crate::plugin::reload_plan;
use crate::plugin::validate_view_spec;
use crate::repo::traits::Repos;
use crate::service::Services;
use crate::update::about::AboutStats;

/// One atomic view of every catalog-derived fact a reader consults. A reload
/// builds a fresh snapshot and swaps it in one step, so no reader sees a new
/// grant beside an old route (ADR-0017).
pub struct PluginSnapshot {
    pub catalog: HashMap<String, CatalogEntry>,
    /// Why the catalog failed to load: owners see the real path and cause
    /// through `/plugins`, plain members keep the friendly empty-catalog view.
    pub catalog_error: Option<InstallError>,
    /// Manifests of the internal plugins spawned at startup, by plugin name.
    pub internal_manifests: HashMap<String, Manifest>,
    /// Command-name → plugin-name routes for dispatch.
    pub routes: PluginRoutes,
    pub grants: AuthoritySnapshot,
    /// A mirror of the framework's command list, rebuilt from the same
    /// snapshot so the command count and Discord registration agree with
    /// dispatch.
    pub commands: Vec<poise::Command<Data, Error>>,
}

/// Data shared across bot commands and contexts.
pub struct Data {
    pub config: Arc<Config>,
    pub service: Arc<Services>,
    pub repos: Arc<dyn Repos + Send + Sync>,
    pub plugin_manager: Arc<PluginManager>,
    /// All catalog-derived state behind one atomic swap. Load a snapshot once
    /// per operation rather than reading fields across two reloads.
    pub plugins: ArcSwap<PluginSnapshot>,
    pub plugin_engine: Arc<InteractionEngine<RunningPlugin>>,
    /// The framework handle is published here by the delegating
    /// [`serenity::Framework`] wrapper during `init`, then used by reloads to
    /// swap the live command list.
    pub framework: Arc<OnceLock<Arc<Framework<Data, Error>>>>,
    /// Tracks the messages owned by live Host (TEA) sessions, so the global
    /// event handler skips their interactions and the Host acknowledges them
    /// exactly once. See [`crate::bot::translate`].
    pub translate_layer: Arc<TranslateLayer>,
    /// The Settings section-handoff return waiters: the Router parks one per
    /// handed-off message, and the host-reserved `settings` open_view target
    /// completes it.
    pub settings_returns: Arc<translate::SettingsReturns>,
    pub start_time: Instant,
}

impl Data {
    /// A single consistent view of the catalog-derived state.
    pub fn snapshot(&self) -> Arc<PluginSnapshot> {
        self.plugins.load_full()
    }

    /// The live framework, published during serenity's `init`.
    pub fn framework(&self) -> &Arc<Framework<Data, Error>> {
        self.framework
            .get()
            .expect("framework handle not published yet")
    }

    /// The names of plugins that auto-enable in every guild: the configured
    /// internal plugins plus catalog entries flagged `auto_enable`.
    pub fn auto_enable_plugins(&self) -> Vec<String> {
        let snapshot = self.snapshot();
        let mut names: Vec<String> = self
            .config
            .internal_plugins
            .iter()
            .map(|spec| spec.name.clone())
            .collect();
        for (name, entry) in snapshot.catalog.iter() {
            if entry.auto_enable && !names.contains(name) {
                names.push(name.clone());
            }
        }
        names.sort();
        names
    }
}

/// The manifest for `name`: the internal plugin's manifest when there is one,
/// else the catalog entry's. The `/plugins` command and registration paths
/// look up by plugin name over the same two maps.
pub(crate) fn manifest_for<'a>(
    internal_manifests: &'a HashMap<String, Manifest>,
    catalog: &'a HashMap<String, CatalogEntry>,
    name: &str,
) -> Option<&'a Manifest> {
    internal_manifests
        .get(name)
        .or_else(|| catalog.get(name).map(|entry| &entry.manifest))
}

/// Serenity owns the framework as a `Box<dyn Framework>` and offers no
/// `Framework for Arc<T>`, so the same framework lives behind an `Arc` handle
/// in [`Data`]. `init` gets `&mut self` while serenity still owns the box
/// exclusively; that is the one chance to run poise's mutable init and
/// publish the `Arc` clone for reloads. `dispatch` then just forwards.
struct FrameworkHandle {
    inner: Arc<Framework<Data, Error>>,
    published: Arc<OnceLock<Arc<Framework<Data, Error>>>>,
}

#[async_trait]
impl poise::serenity_prelude::Framework for FrameworkHandle {
    async fn init(&mut self, client: &poise::serenity_prelude::Client) {
        Arc::get_mut(&mut self.inner)
            .expect("framework handle must be the sole owner during init")
            .init(client)
            .await;
        let _ = self.published.set(Arc::clone(&self.inner));
    }

    async fn dispatch(
        &self,
        ctx: &poise::serenity_prelude::Context,
        event: &poise::serenity_prelude::FullEvent,
    ) {
        self.inner.dispatch(ctx, event).await;
    }
}

/// Discord bot client and framework.
pub struct Bot {
    pub http: Arc<Http>,
    /// Shared with the client and the catalog watcher started in [`Bot::start`].
    data: Arc<Data>,
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
        let settings_returns = Arc::new(translate::SettingsReturns::default());
        let host_services = Arc::new(HostServices {
            io: Some(Arc::new(SerenityHostIo::new(http.clone()))),
            config: Some(HostConfig::from(&*config)),
            kv: Some(Arc::new(PgKvStore::new(repos.plugin_kv()))),
            engine: Some(plugin_engine.clone()),
            stats: stats_handle.clone(),
            users: user_resolver.clone(),
            settings_returns: Some(settings_returns.clone()),
        });
        let plugin_manager = Arc::new(
            PluginManager::new(Some(http.clone()), RespawnPolicy::default())
                .with_host_services(host_services)
                .with_event_bus(event_bus.clone())
                .with_event_router(plugin_events.clone())
                .with_grants(AuthoritySnapshot::from_catalog(
                    &catalog,
                    config.discord_token.clone(),
                )),
        );

        // Internal plugins are spawned once at startup. Their manifest event
        // handlers are subscribed after the handshake; per-guild command
        // registration follows in Ready/GuildCreate. A missing binary is not
        // fatal: the bot stays up and the plugin's commands stay unregistered.
        let mut manifest_sources: Vec<(String, Option<Manifest>)> = Vec::new();
        for spec in &config.internal_plugins {
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
                Err(e) => warn!("failed to spawn internal plugin {}: {e}", spec.name),
            }
        }
        let internal_manifests: HashMap<String, Manifest> = manifest_sources
            .iter()
            .filter_map(|(name, manifest)| manifest.clone().map(|m| (name.clone(), m)))
            .collect();
        let grants = AuthoritySnapshot::from_catalog(&catalog, config.discord_token.clone());
        let snapshot = build_snapshot(catalog, catalog_error, internal_manifests, grants);
        let command_count = AboutStats::count_commands(&snapshot.commands);
        let framework = Arc::new(Self::create_framework(
            &config,
            Self::build_commands(
                &snapshot.internal_manifests,
                &snapshot.catalog,
                &host_command_names(),
            ),
        )?);
        let published = Arc::new(OnceLock::new());

        let start_time = Instant::now();
        let data = Arc::new(Data {
            config: config.clone(),
            service,
            repos,
            plugin_manager,
            plugins: ArcSwap::from_pointee(snapshot),
            plugin_engine,
            framework: published.clone(),
            translate_layer: Arc::new(TranslateLayer::new()),
            settings_returns,
            start_time,
        });

        let stats_source = Arc::new(SerenityStatsSource::new(
            http.clone(),
            start_time,
            config.version.clone(),
            command_count,
        ));
        stats_handle.attach(stats_source.clone());

        let event_handler = Arc::new(BotEventHandler::new(
            data.clone(),
            http.clone(),
            plugin_events,
        ));

        let client_builder = ClientBuilder::new(token.clone(), intents)
            .event_handler(event_handler)
            .framework(Box::new(FrameworkHandle {
                inner: framework,
                published,
            }))
            .data(data.clone())
            .activity(ActivityData::playing(format!(
                "v{}",
                config.version.clone()
            )));

        Ok(Self {
            http,
            data,
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
        let data = self.data.clone();
        let watcher_http = http.clone();
        let watcher_config = data.config.clone();

        tokio::spawn(async move {
            info!("Connecting bot to Discord...");

            let built_client = client_builder
                .await
                .expect("Failed to build Discord client");

            // The framework's `init` has published the Arc handle by now, so
            // a catalog edit can swap commands immediately after startup.
            start_catalog_watcher(watcher_config, data, watcher_http);

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

    /// Builds the full command list from a snapshot's plugin facts. Called
    /// twice per reload: once for the framework's dispatch list, once for the
    /// snapshot's mirror used by the command count and Discord registration.
    fn build_commands(
        internal_manifests: &HashMap<String, Manifest>,
        catalog: &HashMap<String, CatalogEntry>,
        host_command_names: &HashSet<String>,
    ) -> Vec<poise::Command<Data, Error>> {
        let mut commands = Cogs.commands();
        commands.extend(plugin_commands(
            internal_manifests,
            catalog,
            host_command_names,
        ));
        commands
    }

    /// Creates the Poise framework with commands and configuration. The
    /// command list is the merge seam; see [`Self::build_commands`].
    fn create_framework(
        config: &Config,
        commands: Vec<poise::Command<Data, Error>>,
    ) -> Result<Framework<Data, Error>> {
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

        Ok(poise::Framework::builder().options(options).build())
    }

    /// Loads the plugin catalog from `plugins.toml`. A missing or invalid
    /// catalog keeps the bot up with plugin commands absent rather than
    /// failing startup, but the failure is logged loudly and returned so
    /// owners see the real path and cause through the `/plugins`
    /// commands ([`PluginSnapshot::catalog_error`]).
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

/// The plugin routing commands for the framework: internal plugin manifests
/// first, then catalog plugin manifests, each group sorted by plugin name
/// so the assembled command order is stable across restarts.
///
/// Host Cog roots and earlier plugin roots are reserved. A catalog entry
/// whose plugin also runs as an internal plugin contributes nothing: its commands
/// come from the internal manifest only. Catalog entries for internal plugins exist so
/// `/plugins list` and the install/update sources see them.
fn plugin_commands(
    internal_manifests: &HashMap<String, Manifest>,
    catalog: &HashMap<String, CatalogEntry>,
    reserved_names: &HashSet<String>,
) -> Vec<poise::Command<Data, Error>> {
    let mut commands = Vec::new();
    let mut registered_names = HashSet::new();
    let mut core: Vec<&Manifest> = internal_manifests.values().collect();
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
        if internal_manifests.contains_key(&entry.name) {
            warn!(
                "skipping catalog commands for `{}`: it is an internal plugin, its commands come from the internal manifest",
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

/// Builds one self-consistent snapshot from a catalog and the manifests
/// captured at plugin spawn. Shared by startup, reload, and the snapshot tests
/// so the routes, grants, and command list always derive from the same facts.
fn build_snapshot(
    catalog: HashMap<String, CatalogEntry>,
    catalog_error: Option<InstallError>,
    internal_manifests: HashMap<String, Manifest>,
    grants: AuthoritySnapshot,
) -> PluginSnapshot {
    let mut route_sources: Vec<(String, Option<Manifest>)> = internal_manifests
        .iter()
        .map(|(name, manifest)| (name.clone(), Some(manifest.clone())))
        .collect();
    for (name, entry) in &catalog {
        route_sources.push((name.clone(), Some(entry.manifest.clone())));
    }
    route_sources.sort_by(|(left_name, _), (right_name, _)| {
        let left_is_catalog = !internal_manifests.contains_key(left_name);
        let right_is_catalog = !internal_manifests.contains_key(right_name);
        right_is_catalog
            .cmp(&left_is_catalog)
            .then_with(|| left_name.cmp(right_name))
    });
    let host_names = host_command_names();
    let routes = routes_from_manifests_with_reserved(route_sources, &host_names);
    let commands = Bot::build_commands(&internal_manifests, &catalog, &host_names);
    PluginSnapshot {
        catalog,
        catalog_error,
        internal_manifests,
        routes,
        grants,
        commands,
    }
}

const CATALOG_POLL_INTERVAL: Duration = Duration::from_secs(2);
const CATALOG_DEBOUNCE: Duration = Duration::from_millis(400);

fn catalog_mtime(path: &Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path)
        .ok()
        .and_then(|meta| meta.modified().ok())
}

/// Polls `plugins.toml` for a settled mtime change. One task handles every
/// reload, so two rapid writes cannot interleave a spawn with a swap. Polling
/// avoids a watcher dependency; the quiet period absorbs the several writes an
/// editor or the operator CLI makes for one logical change.
fn start_catalog_watcher(config: Arc<Config>, data: Arc<Data>, http: Arc<Http>) {
    let path = config.plugins_toml.clone();
    tokio::spawn(async move {
        let mut last = catalog_mtime(&path);
        let mut ticker = tokio::time::interval(CATALOG_POLL_INTERVAL);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;
            let seen = catalog_mtime(&path);
            if seen == last {
                continue;
            }
            tokio::time::sleep(CATALOG_DEBOUNCE).await;
            let settled = catalog_mtime(&path);
            if settled != seen {
                continue;
            }
            match apply_catalog_reload(&config, &data, &http).await {
                Ok(()) => last = settled,
                Err(e) => warn!("plugin catalog reload failed: {e}; will retry the same change"),
            }
        }
    });
}

/// Rejects an entry whose operator grant and manifest declaration disagree.
/// Direct edits to `plugins.toml` are supported, so the reload path must run
/// the same both-directions check the CLI add path runs.
fn validate_grant_agreement(
    catalog: &HashMap<String, CatalogEntry>,
) -> Result<(), (String, crate::plugin::catalog_writer::GrantMismatch)> {
    for (name, entry) in catalog {
        check_grant(entry.discord_token, &entry.manifest)
            .map_err(|mismatch| (name.clone(), mismatch))?;
    }
    Ok(())
}

/// Performs every fallible process operation before any state is published.
/// A failed install or spawn leaves grants, snapshot, and commands untouched;
/// plugins started before the failure are stopped so no child keeps a token
/// the committed snapshot does not authorize.
async fn prepare_processes(
    config: &Config,
    data: &Data,
    catalog: &HashMap<String, CatalogEntry>,
    grants: &AuthoritySnapshot,
    plan: &ReloadPlan,
) -> Result<(), Error> {
    let client = crate::plugin::install::download_client();
    let mut started: Vec<String> = Vec::new();
    let mut replaced_down: Vec<String> = Vec::new();
    let result = async {
        for name in plan.spawn.iter().chain(plan.replace.iter()) {
            let entry = &catalog[name];
            let authority = grants.resolve(name)?;
            let path = install_verified(&client, entry, &config.plugins_dir).await?;
            if plan.replace.contains(name) {
                match data.plugin_manager.unload(name, &[]).await {
                    Ok(_) => replaced_down.push(name.clone()),
                    Err(PluginError::NotRunning { .. }) => {}
                    Err(e) => {
                        warn!("hot reload could not unload `{name}` before replacing it: {e}")
                    }
                }
            }
            data.plugin_manager
                .spawn_with_authority(
                    name,
                    path,
                    None,
                    &entry.manifest.event_handlers,
                    &entry.manifest.tasks,
                    authority,
                )
                .await?;
            started.push(name.clone());
            info!("hot reload started catalog plugin `{name}`");
        }
        for name in &plan.unload {
            match data.plugin_manager.unload(name, &[]).await {
                Ok(_) => {
                    info!(
                        "hot reload unloaded removed catalog plugin `{name}`; binary left on disk"
                    )
                }
                Err(PluginError::NotRunning { .. }) => {}
                Err(e) => return Err(e.into()),
            }
        }
        Ok::<(), Error>(())
    }
    .await;

    if result.is_err() {
        for name in &started {
            if let Err(e) = data.plugin_manager.unload(name, &[]).await {
                warn!("rollback could not stop newly started plugin `{name}`: {e}");
            }
        }
        for name in replaced_down.iter().filter(|name| !started.contains(name)) {
            warn!(
                "plugin `{name}` was unloaded for replacement and could not be restarted; it is down while the command list still advertises it"
            );
        }
    }
    result
}

/// Applies one settled catalog change: validate, finish all fallible process
/// I/O, then publish grants, snapshot, and commands and register with Discord.
async fn apply_catalog_reload(config: &Config, data: &Data, http: &Http) -> Result<(), Error> {
    let (catalog, catalog_error) = Bot::load_plugin_catalog(config);
    let previous = data.snapshot();
    let internal_names: HashSet<String> = config
        .internal_plugins
        .iter()
        .map(|spec| spec.name.clone())
        .collect();
    let plan: ReloadPlan = reload_plan(&previous.catalog, &catalog, &internal_names);
    let grants = AuthoritySnapshot::from_catalog(&catalog, config.discord_token.clone());

    if let Err((name, mismatch)) = validate_grant_agreement(&catalog) {
        warn!(
            "plugin catalog reload rejected `{name}`: grant and declaration disagree: {mismatch}"
        );
        return Err(anyhow::anyhow!("plugin `{name}`: {mismatch}").into());
    }

    prepare_processes(config, data, &catalog, &grants, &plan).await?;

    let entry_count = catalog.len();
    let snapshot = build_snapshot(
        catalog,
        catalog_error,
        previous.internal_manifests.clone(),
        grants.clone(),
    );
    let framework = data.framework().clone();
    let host_names = host_command_names();
    data.plugin_manager.set_grants(grants);
    info!("plugin grant snapshot rebuilt from the reloaded catalog: {entry_count} entries");

    let _ = data.plugins.swap(Arc::new(snapshot));
    let current = data.snapshot();
    let poise_commands =
        Bot::build_commands(&current.internal_manifests, &current.catalog, &host_names);
    let _previous_commands = framework.replace_commands(poise_commands);
    info!(
        "plugin catalog reload applied locally: {} commands",
        current.commands.len()
    );

    if config.features.autoregister_cmds {
        if let Err(e) = poise::builtins::register_globally(http, &current.commands).await {
            let rollback_commands =
                Bot::build_commands(&previous.internal_manifests, &previous.catalog, &host_names);
            let _discarded = framework.replace_commands(rollback_commands);
            data.plugin_manager.set_grants(previous.grants.clone());
            if let Err(register_err) =
                poise::builtins::register_globally(http, &previous.commands).await
            {
                warn!(
                    "rollback could not restore the previous commands on Discord: {register_err}"
                );
            } else {
                info!("rollback restored the previous commands on Discord");
            }
            data.plugins.store(previous);
            warn!("plugin catalog reload could not register with Discord ({e}); rolled back");
            return Err(e.into());
        }
        info!(
            "plugin catalog reload registered {} commands with Discord",
            current.commands.len()
        );
    }
    Ok(())
}

/// What a plugin's answer to a view interaction means for the message it
/// edits. See [`BotEventHandler::view_answer`].
enum ViewAnswer {
    /// The fresh render to send: the edit body plus the runtime files the
    /// spec ships.
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

                let snapshot = self.data.snapshot();
                let mut commands = Cogs.commands();
                commands.extend(plugin_commands(
                    &snapshot.internal_manifests,
                    &snapshot.catalog,
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
        let snapshot = self.data.snapshot();
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
            let Some(manifest) = manifest_for(
                &snapshot.internal_manifests,
                &snapshot.catalog,
                &plugin_name,
            ) else {
                let catalog_suffix = match snapshot.catalog_error.as_ref() {
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
        guild_id: Option<GuildId>,
        custom_id: &str,
        interaction: Value,
        interaction_token: &str,
        kind: &str,
    ) {
        if !self.session_plugin_enabled(message_id, guild_id).await {
            self.drop_disabled_view(message_id).await;
            return;
        }

        let result = self
            .data
            .plugin_engine
            .interact_validated(message_id, custom_id, interaction, validate_view_spec)
            .await;

        if let ViewAnswer::Render(body, files) = self.view_answer(result, message_id, kind).await
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
    /// message: the fresh render to send, nothing because the plugin
    /// answered the interaction itself, or nothing because the view is
    /// stale or the session failed.
    async fn view_answer(
        &self,
        result: Result<ViewSpec, InteractionError>,
        message_id: MessageId,
        kind: &str,
    ) -> ViewAnswer {
        match result {
            // The plugin's payload is a create envelope: the edit transport
            // strips the create-only fields Discord rejects on edit (error
            // 50080 for `sticker_ids`) before the body is sent.
            Ok(spec) => {
                let body = edit_body_for_transport(&spec.data);
                let files = match decode_runtime_files(&spec.files) {
                    Ok(files) => files,
                    Err(error) => {
                        warn!("plugin returned invalid runtime files: {}", error.msg);
                        return ViewAnswer::Nothing;
                    }
                };
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

        if !self
            .session_plugin_enabled(message_id, interaction.guild_id)
            .await
        {
            self.ack_component(interaction, message_id).await;
            self.drop_disabled_view(message_id).await;
            return;
        }

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
                .view_answer(result, message_id, "component interaction")
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
                    .view_answer(result, message_id, "component interaction")
                    .await
                {
                    self.edit_after_ack(message_id, interaction.token.as_str(), body, files)
                        .await;
                }
            }
        }
    }

    /// Whether the plugin behind `message_id`'s open view session may still
    /// serve `guild_id`.
    ///
    /// The session records the command it was opened from, and the route
    /// table names that command's plugin, so a plugin switched off *after* its
    /// view was rendered stops serving the clicks too — the second half of the
    /// per-guild disable, next to the command gate in
    /// [`open_plugin_view`](crate::plugin::command::open_plugin_view). No
    /// session or no route means routing already refuses the interaction, and
    /// a DM carries no guild, so there is no per-guild state to gate either
    /// way.
    ///
    /// A row read that fails serves the view anyway, with the cause in the
    /// log: the panel is already on screen and answered by the author, and a
    /// database blip must not freeze every open panel.
    async fn session_plugin_enabled(
        &self,
        message_id: MessageId,
        guild_id: Option<GuildId>,
    ) -> bool {
        let data = &self.data;
        let Some(command) = data.plugin_engine.session_command(message_id).await else {
            return true;
        };
        let Some(plugin) = data.snapshot().routes.get(&command).cloned() else {
            return true;
        };
        match data.plugin_enabled_in(guild_id, &plugin).await {
            Ok(enabled) => enabled,
            Err(e) => {
                warn!("failed to read the guild plugin rows for `{plugin}`: {e}");
                true
            }
        }
    }

    /// Drops the view session for a plugin this guild switched off. The panel
    /// on screen is stale, so the session goes the way a stale session does:
    /// `view.timeout` and dropped, leaving the click acknowledged.
    async fn drop_disabled_view(&self, message_id: MessageId) {
        debug!("plugin view {message_id} is disabled in this guild; dropping the session");
        if let Err(e) = self.data.plugin_engine.abandon(message_id).await {
            warn!("failed to abandon session for message {message_id}: {e}");
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

        // A plugin this guild switched off since the modal was opened must
        // not have its write land: the gate consumes the route and refuses,
        // so the same submit cannot be retried. Checked before delivery,
        // which would otherwise take the route and answer.
        if let Some(plugin) = self
            .data
            .take_disabled_modal(interaction.user.id.get(), interaction.guild_id)
            .await
        {
            self.refuse_disabled_modal_submission(interaction, &plugin)
                .await;
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
            interaction.guild_id,
            &interaction.data.custom_id,
            serde_json::to_value(interaction).unwrap_or_default(),
            interaction.token.as_str(),
            "modal submit",
        )
        .await;
    }

    /// Answers a submission of a modal whose plugin this guild switched off:
    /// the same refusal a disabled plugin command gives, sent as the
    /// submission's own response, privately — it concerns one author's
    /// submission. The route is already consumed by
    /// [`take_disabled_modal`](Data::take_disabled_modal), so the answer is
    /// all that is left for this submission.
    async fn refuse_disabled_modal_submission(&self, interaction: &ModalInteraction, plugin: &str) {
        debug!("plugin `{plugin}` is disabled in this guild; refusing its modal submission");
        let refusal = BotError::PluginDisabledInGuild {
            plugin: plugin.to_string(),
        };
        let answer = CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new()
                .content(refusal.to_string())
                .ephemeral(true),
        );
        if let Err(e) = interaction.create_response(&self.http, answer).await {
            warn!("failed to refuse a modal submission of disabled plugin `{plugin}`: {e}");
        }
    }

    /// Renders a plugin's answer to its own modal submission as the
    /// submission's response: an update-message carrying the answer's
    /// create envelope through the edit-transport strip (the same
    /// projection every edit sends). A submission opened from a component
    /// click replaces that message; one opened from a command answers as a
    /// fresh message, honoring the spec's ephemerality.
    async fn render_modal_submission_response(
        &self,
        interaction: &ModalInteraction,
        spec: &ViewSpec,
    ) {
        let kind = if interaction.message.is_some() { 7 } else { 4 };
        let mut data = edit_body_for_transport(&spec.data);
        let files = match decode_runtime_files(&spec.files) {
            Ok(files) => files,
            Err(error) => {
                warn!("plugin returned invalid runtime files: {}", error.msg);
                return;
            }
        };
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

    #[test]
    fn a_snapshot_swap_publishes_catalog_routes_grants_and_commands_together() {
        let host_command_count = host_command_names().len();
        let store = ArcSwap::from_pointee(build_snapshot(
            HashMap::new(),
            None,
            HashMap::new(),
            AuthoritySnapshot::none(),
        ));

        let catalog = HashMap::from([("voice".to_string(), entry_named("voice"))]);
        let grants = AuthoritySnapshot::from_catalog(&catalog, "bot-token".to_string());
        let fresh = build_snapshot(catalog, None, HashMap::new(), grants);
        let old = store.swap(Arc::new(fresh));
        let current = store.load_full();

        assert!(old.catalog.is_empty());
        assert_eq!(current.catalog.len(), 1);
        assert_eq!(
            current.routes.get("voice").map(String::as_str),
            Some("voice")
        );
        assert!(current.grants.resolve("voice").is_ok());
        assert_eq!(current.commands.len(), host_command_count + 1);
    }

    #[test]
    fn reload_rejects_a_grant_without_a_manifest_declaration() {
        let mut entry = entry_named("voice");
        entry.discord_token = true;
        let catalog = HashMap::from([("voice".to_string(), entry)]);
        assert!(validate_grant_agreement(&catalog).is_err());
    }

    #[test]
    fn reload_rejects_a_declaration_without_an_operator_grant() {
        let mut entry = entry_named("voice");
        entry.manifest.requires = vec![pwr_plugin_protocol::DISCORD_TOKEN.into()];
        let catalog = HashMap::from([("voice".to_string(), entry)]);
        assert!(validate_grant_agreement(&catalog).is_err());
    }

    #[test]
    fn reload_accepts_an_entry_whose_grant_and_declaration_agree() {
        let mut entry = entry_named("voice");
        entry.discord_token = true;
        entry.manifest.requires = vec![pwr_plugin_protocol::DISCORD_TOKEN.into()];
        let catalog = HashMap::from([("voice".to_string(), entry)]);
        assert!(validate_grant_agreement(&catalog).is_ok());
    }

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
            plugins: ArcSwap::from_pointee(PluginSnapshot {
                catalog: HashMap::new(),
                catalog_error: None,
                internal_manifests: HashMap::new(),
                routes: HashMap::new(),
                grants: AuthoritySnapshot::none(),
                commands: Vec::new(),
            }),
            plugin_engine: engine.clone(),
            framework: Arc::new(OnceLock::new()),
            translate_layer: Arc::new(TranslateLayer::new()),
            settings_returns: Arc::new(crate::bot::translate::SettingsReturns::default()),
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
        let internal_manifests = HashMap::from([
            ("zeta".to_string(), manifest_named("zeta")),
            ("alpha".to_string(), manifest_named("alpha")),
        ]);
        let catalog = HashMap::from([
            ("mike".to_string(), entry_named("mike")),
            ("bravo".to_string(), entry_named("bravo")),
        ]);

        let commands = plugin_commands(&internal_manifests, &catalog, &HashSet::new());
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
        let internal_manifests = HashMap::from([("plugin".to_string(), manifest.clone())]);
        let reserved = host_command_names();
        let routes = routes_from_manifests_with_reserved(
            [("plugin".to_string(), Some(manifest))],
            &reserved,
        );
        let commands = plugin_commands(&internal_manifests, &HashMap::new(), &reserved);

        assert!(!routes.contains_key("settings"));
        assert!(
            commands
                .iter()
                .all(|command| command.name.as_ref() != "settings")
        );
    }

    /// A catalog entry for a plugin that also runs as an internal plugin is
    /// skipped: the internal manifest is the only source for its commands, so
    /// `set_commands` never sees the same command name twice.
    #[test]
    fn plugin_commands_skip_a_catalog_entry_that_duplicates_a_internal_plugin() {
        let internal_manifests =
            HashMap::from([("settings".to_string(), manifest_named("settings"))]);
        let catalog = HashMap::from([("settings".to_string(), entry_named("settings"))]);

        let commands = plugin_commands(&internal_manifests, &catalog, &HashSet::new());
        let names: Vec<&str> = commands
            .iter()
            .map(|command| command.name.as_ref())
            .collect();

        assert_eq!(names, ["settings"]);
    }
    /// A catalog entry whose plugin is not an internal plugin still contributes
    /// its commands.
    #[test]
    fn plugin_commands_keep_a_catalog_entry_whose_plugin_is_not_core() {
        let internal_manifests =
            HashMap::from([("settings".to_string(), manifest_named("settings"))]);
        let catalog = HashMap::from([("greet".to_string(), entry_named("greet"))]);

        let commands = plugin_commands(&internal_manifests, &catalog, &HashSet::new());
        let names: Vec<&str> = commands
            .iter()
            .map(|command| command.name.as_ref())
            .collect();

        assert_eq!(names, ["settings", "greet"]);
    }

    /// The full registered command surface — the host Cog commands plus one
    /// routing command per internal plugin manifest, assembled through the same
    /// `commands_from_manifest` path the host registers through — pinned
    /// against a committed snapshot. With `UPDATE_SNAPSHOT=1` the test
    /// rewrites the fixture instead of asserting.
    #[test]
    fn the_registered_command_surface_matches_the_committed_snapshot() {
        let manifest_fixture = fs::read_to_string(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/internal_manifests.json"),
        )
        .expect("internal manifest fixture is readable");
        let manifests: Vec<Manifest> =
            serde_json::from_str(&manifest_fixture).expect("internal manifest fixture is valid");
        let internal_manifests = manifests
            .into_iter()
            .map(|manifest| (manifest.name.clone(), manifest))
            .collect();
        let mut commands = Cogs.commands();
        commands.extend(plugin_commands(
            &internal_manifests,
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
