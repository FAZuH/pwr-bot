//! Application entry point for pwr-bot.
//!
//! Initializes all components and starts the Discord bot.

use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use dotenv::dotenv;
use log::debug;
use log::info;
use pwr_bot::bot::Bot;
use pwr_bot::config::Config;
use pwr_bot::bot::plugin::host_registry;
use pwr_bot::bot::host_ctx::PoiseHostCtx;
use pwr_bot::event::FeedUpdateEvent;
use pwr_bot::event::event_bus::EventBus;
use pwr_bot::logging::setup_logging;
use pwr_bot::repo::PgRepos;
use pwr_bot::repo::traits::Repos;
use pwr_bot::service::Services;
use pwr_bot::bot::plugin::invocation;
use pwr_bot_plugin_feed::FeedPlugin;
use pwr_bot_plugin_voice::VoicePlugin;

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();

    let init_start = Instant::now();
    let config = load_config().await?;
    let event_bus = Arc::new(EventBus::new());

    let repos = setup_database(&config, init_start).await?;
    let services = setup_services(repos.clone()).await?;

    let bot = setup_bot(
        &config,
        event_bus.clone(),
        services.clone(),
        init_start,
    )
    .await?;

    // Initialize system context for headless plugin operations (events, tasks)
    host_registry::set_system_ctx(PoiseHostCtx::new_system(
        bot.data.clone(),
        bot.http.clone(),
    ));

    // Register builtin plugins and run their init hooks
    let voice_plugin = Arc::new(VoicePlugin::new()) as Arc<dyn pwr_bot_sdk::BotPlugin>;
    invocation::dispatch_init(&*voice_plugin)
        .await
        .map_err(|e| anyhow::anyhow!("Voice plugin init failed: {e}"))?;

    let feed_plugin = Arc::new(FeedPlugin::new()) as Arc<dyn pwr_bot_sdk::BotPlugin>;
    invocation::dispatch_init(&*feed_plugin)
        .await
        .map_err(|e| anyhow::anyhow!("Feed plugin init failed: {e}"))?;

    let plugins: Vec<Arc<dyn pwr_bot_sdk::BotPlugin>> = vec![voice_plugin.clone(), feed_plugin.clone()];

    // Bridge typed FeedUpdateEvent to named event for plugins
    {
        let bus = event_bus.clone();
        event_bus.register_callback(move |event: FeedUpdateEvent| {
            let bus = bus.clone();
            async move {
                if let Ok(json) = serde_json::to_value(&event) {
                    let _ = bus.publish_named("feed_update", json);
                }
                Ok(())
            }
        });
    }

    // Register plugin event handlers on the event bus
    invocation::register_plugin_event_handlers(&event_bus, &plugins);

    // Spawn background tasks declared by plugins
    invocation::dispatch_tasks(&plugins).await;

    info!(
        "pwr-bot is up in {:.2}s. Press Ctrl+C to stop.",
        init_start.elapsed().as_secs_f64()
    );
    tokio::signal::ctrl_c().await?;
    info!("Ctrl+C received, shutting down.");

    Ok(())
}

async fn load_config() -> Result<Arc<Config>> {
    debug!("Loading configuration...");
    let mut config = Config::new();
    config.load()?;
    let config = Arc::new(config);
    setup_logging(&config)?;
    info!("Starting pwr-bot...");
    Ok(config)
}

async fn setup_database(
    config: &Config,
    init_start: Instant,
) -> Result<Arc<dyn Repos + Send + Sync>> {
    debug!("Setting up Database...");
    let repos = PgRepos::new(&config.db_url).await?;

    info!("Running database migrations...");
    repos.run_migrations().await?;
    info!(
        "Database setup complete ({:.2}s).",
        init_start.elapsed().as_secs_f64()
    );

    Ok(Arc::new(repos))
}

async fn setup_services(
    repos: Arc<dyn Repos + Send + Sync>,
) -> Result<Arc<Services>> {
    debug!("Setting up Services...");
    Ok(Arc::new(Services::new(repos).await?))
}

async fn setup_bot(
    config: &Arc<Config>,
    event_bus: Arc<EventBus>,
    services: Arc<Services>,
    init_start: Instant,
) -> Result<Arc<Bot>> {
    info!("Starting bot...");
    let mut bot = Bot::new(
        config.clone(),
        event_bus,
        services,
    )
    .await?;

    bot.start();
    let bot = Arc::new(bot);
    info!(
        "Bot setup complete ({:.2}s).",
        init_start.elapsed().as_secs_f64()
    );

    Ok(bot)
}
