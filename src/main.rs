//! Application entry point for pwr-bot.
//!
//! Initializes all components and starts the Discord bot.

use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use dotenv::dotenv;
use tracing::debug;
use tracing::info;
use pwr_bot::bot::Bot;
use pwr_bot::bot::host_ctx::PoiseHostCtx;
use pwr_bot::bot::plugin::host_registry;
use pwr_bot::bot::plugin::invocation;
use pwr_bot::bot::plugin::loader;
use pwr_bot::bot::plugin::registry::PluginRegistry;
use pwr_bot::config::Config;
use pwr_bot::event::event_bus::EventBus;
use pwr_bot::logging::setup_logging;
use pwr_bot::repo::PgRepos;
use pwr_bot::repo::traits::Repos;
use pwr_bot::service::Services;

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();

    let init_start = Instant::now();
    let config = load_config().await?;
    let event_bus = Arc::new(EventBus::new());

    let repos = setup_database(&config, init_start).await?;
    let services = setup_services(repos.clone()).await?;

    // Create plugin registry and register all plugins
    let registry = Arc::new(PluginRegistry::new());

    // Load FFI (.so) plugins from plugin directory
    let ffi_plugins = loader::load_plugins(&config.plugin_dir);
    for ffi_plugin in ffi_plugins {
        registry.register(ffi_plugin).await;
    }

    let bot = setup_bot(
        &config,
        event_bus.clone(),
        services.clone(),
        registry.clone(),
        init_start,
    )
    .await?;

    // Initialize system context for headless plugin operations (events, tasks)
    host_registry::set_system_ctx(PoiseHostCtx::new_system(bot.data.clone(), bot.http.clone()));

    // Initialize all plugins
    let plugins = registry.all_ffi_plugins().await;
    for plugin in &plugins {
        invocation::dispatch_init_ffi(plugin)
            .await
            .map_err(|e| anyhow::anyhow!("{} plugin init failed: {e}", plugin.name))?;
    }

    // Register plugin event handlers on the event bus
    invocation::register_ffi_event_handlers(&event_bus, &plugins);

    // Spawn background tasks declared by plugins
    invocation::dispatch_tasks_ffi(&plugins).await;

    info!(
        duration = init_start.elapsed().as_secs_f64(),
        "pwr-bot is up",
    );
    tokio::signal::ctrl_c().await?;
    info!("shutting down");

    Ok(())
}

async fn load_config() -> Result<Arc<Config>> {
    debug!("loading configuration");
    let mut config = Config::new();
    config.load()?;
    let config = Arc::new(config);
    setup_logging(&config)?;
    info!("starting pwr-bot");
    Ok(config)
}

async fn setup_database(
    config: &Config,
    init_start: Instant,
) -> Result<Arc<dyn Repos + Send + Sync>> {
    debug!("setting up database");
    let repos = PgRepos::new(&config.db_url).await?;

    info!("running database migrations");
    repos.run_migrations().await?;
    info!(
        duration = init_start.elapsed().as_secs_f64(),
        "database setup complete",
    );

    Ok(Arc::new(repos))
}

async fn setup_services(repos: Arc<dyn Repos + Send + Sync>) -> Result<Arc<Services>> {
    debug!("setting up services");
    Ok(Arc::new(Services::new(repos).await?))
}

async fn setup_bot(
    config: &Arc<Config>,
    event_bus: Arc<EventBus>,
    services: Arc<Services>,
    plugin_registry: Arc<PluginRegistry>,
    init_start: Instant,
) -> Result<Arc<Bot>> {
    info!("starting bot");
    let mut bot = Bot::new(config.clone(), event_bus, services, plugin_registry).await?;

    bot.start();
    let bot = Arc::new(bot);
    info!(
        duration = init_start.elapsed().as_secs_f64(),
        "bot setup complete",
    );

    Ok(bot)
}
