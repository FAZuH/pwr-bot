pub mod bot;
pub mod config;
pub mod database;
pub mod error;
pub mod event;
pub mod feed;
pub mod logging;
pub mod publisher;
pub mod service;
pub mod subscriber;

use std::sync::Arc;
use std::time::Instant;

use dotenv::dotenv;
use log::debug;
use log::info;

use crate::bot::Bot;
use crate::config::Config;
use crate::database::Database;
use crate::event::event_bus::EventBus;
use crate::feed::platforms::Platforms;
use crate::logging::setup_logging;
use crate::publisher::series_feed_publisher::SeriesFeedPublisher;
use crate::service::Services;
use crate::subscriber::discord_dm_subscriber::DiscordDmSubscriber;
use crate::subscriber::discord_guild_subscriber::DiscordGuildSubscriber;
use crate::subscriber::voice_state_subscriber::VoiceStateSubscriber;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let init_start = Instant::now();
    dotenv().ok();

    info!("Starting pwr-bot...");

    debug!("Setting up Config...");
    let mut config = Config::new();
    config.load()?;
    let config = Arc::new(config);

    debug!("Setting up Config...");
    setup_logging(&config)?;

    debug!("Setting up EventBus...");
    let event_bus = Arc::new(EventBus::new());

    // Setup database
    debug!("Setting up Database...");
    let db = Arc::new(Database::new(&config.db_url, &config.db_path).await?);
    info!("Running database migrations...");
    db.run_migrations().await?;
    info!(
        "Database setup complete ({:.2}s).",
        init_start.elapsed().as_secs_f64()
    );

    // Setup sources
    debug!("Setting up Platforms...");
    let platforms = Arc::new(Platforms::new());

    debug!("Setting up Services...");
    let services = Arc::new(Services::new(db.clone(), platforms.clone()));

    // Setup & start bot
    info!("Starting bot...");
    let mut bot = Bot::new(
        config.clone(),
        db.clone(),
        event_bus.clone(),
        platforms.clone(),
        services.clone(),
    )
    .await?;
    bot.start();
    let bot = Arc::new(bot);
    info!(
        "Bot setup complete ({:.2}s).",
        init_start.elapsed().as_secs_f64()
    );

    // Setup subscribers
    debug!("Setting up Subscribers...");
    event_bus
        .register_subcriber(DiscordDmSubscriber::new(bot.clone(), db.clone()).into())
        .register_subcriber(DiscordGuildSubscriber::new(bot.clone(), db.clone()).into())
        .register_subcriber(VoiceStateSubscriber::new(services.clone()).into());
    info!(
        "Subscribers setup complete ({:.2}s).",
        init_start.elapsed().as_secs_f64()
    );

    // Setup publishers
    debug!("Setting up Publishers...");
    SeriesFeedPublisher::new(
        services.feed_subscription.clone(),
        event_bus.clone(),
        config.poll_interval,
    )
    .start()?;
    info!(
        "Publishers setup complete ({:.2}s).",
        init_start.elapsed().as_secs_f64()
    );

    // Listen for exit signal
    let init_done = init_start.elapsed();
    info!(
        "pwr-bot is up in {:.2}s. Press Ctrl+C to stop.",
        init_done.as_secs_f64()
    );
    tokio::signal::ctrl_c().await?;
    info!("Ctrl+C received, shutting down.");
    // Stop publishers

    Ok(())
}
