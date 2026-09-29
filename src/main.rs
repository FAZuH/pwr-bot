//! Application entry point for pwr-bot.
//!
//! Initializes the host database, starts the bot, and lets core plugins own
//! their domains.

use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use dotenv::dotenv;
use log::debug;
use log::info;
use pwr_bot::bot::Bot;
use pwr_bot::config::Config;
use pwr_bot::event::event_bus::EventBus;
use pwr_bot::logging::setup_logging;
use pwr_bot::repo::PgRepos;
use pwr_bot::repo::traits::Repos;
use pwr_bot::service::Services;

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("no other rustls CryptoProvider installed yet");

    let init_start = Instant::now();
    let config = load_config().await?;
    let event_bus = Arc::new(EventBus::new());
    let repos = setup_database(&config, init_start).await?;
    let services = setup_services(repos.clone()).await?;
    setup_bot(&config, event_bus, services, repos, init_start).await?;
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

async fn setup_services(repos: Arc<dyn Repos + Send + Sync>) -> Result<Arc<Services>> {
    debug!("Setting up Services...");
    Ok(Arc::new(Services::new(repos).await?))
}

async fn setup_bot(
    config: &Arc<Config>,
    event_bus: Arc<EventBus>,
    services: Arc<Services>,
    repos: Arc<dyn Repos + Send + Sync>,
    init_start: Instant,
) -> Result<Arc<Bot>> {
    info!("Starting bot...");
    let mut bot = Bot::new(config.clone(), event_bus, services, repos).await?;
    bot.start();
    let bot = Arc::new(bot);
    info!(
        "Bot setup complete ({:.2}s).",
        init_start.elapsed().as_secs_f64()
    );
    Ok(bot)
}
