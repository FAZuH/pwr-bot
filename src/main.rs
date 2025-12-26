pub mod bot;
pub mod config;
pub mod database;
pub mod error;
pub mod event;
pub mod feed;
pub mod publisher;
pub mod subscriber;

use std::sync::Arc;

use dotenv::dotenv;
use log::debug;
use log::info;

use crate::bot::Bot;
use crate::config::Config;
use crate::database::Database;
use crate::event::event_bus::EventBus;
use crate::feed::feeds::Feeds;
use crate::publisher::series_feed_publisher::SeriesFeedPublisher;
use crate::subscriber::discord_channel_subscriber::DiscordChannelSubscriber;
use crate::subscriber::discord_dm_subscriber::DiscordDmSubscriber;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    env_logger::init();
    info!("Starting pwr-bot...");

    debug!("Instantiating Config...");
    let mut config = Config::new();
    config.load()?;
    let config = Arc::new(config);

    debug!("Instantiating EventBus...");
    let event_bus = Arc::new(EventBus::new());

    // Setup database
    debug!("Instantiating Database...");
    let db = Arc::new(Database::new(&config.db_url, &config.db_path).await?);
    info!("Running database migrations...");
    db.run_migrations().await?;
    info!("Database setup complete.");

    // Setup sources
    debug!("Instantiating Sources...");
    let feeds = Arc::new(Feeds::new());

    // Setup & start bot
    info!("Starting bot...");
    let mut bot = Bot::new(config.clone(), db.clone(), feeds.clone()).await?;
    bot.start();
    let bot = Arc::new(bot);
    info!("Bot setup complete.");

    // Setup subscribers
    debug!("Instantiating Subscribers...");
    let dm_subscriber = DiscordDmSubscriber::new(bot.clone(), db.clone());
    let webhook_subscriber = DiscordChannelSubscriber::new(bot.clone(), db.clone());
    event_bus
        .register_subcriber(dm_subscriber.into())
        .register_subcriber(webhook_subscriber.into());
    info!("Subscribers setup complete.");

    // Setup publishers
    debug!("Instantiating Publishers...");
    SeriesFeedPublisher::new(
        db.clone(),
        event_bus.clone(),
        feeds.clone(),
        config.poll_interval,
    )
    .start()?;
    info!("Publishers setup complete.");

    // Listen for exit signal
    info!("pwr-bot is up. Press Ctrl+C to stop.");
    tokio::signal::ctrl_c().await?;
    info!("Ctrl+C received, shutting down.");
    // Stop publishers

    Ok(())
}
