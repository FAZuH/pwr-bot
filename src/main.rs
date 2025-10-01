pub mod bot;
pub mod config;
pub mod database;
pub mod event;
pub mod publisher;
pub mod source;
pub mod subscriber;

use crate::bot::bot::Bot;
use crate::config::Config;
use crate::database::database::Database;
use crate::event::event_bus::EventBus;
use crate::event::feed_update_event::FeedUpdateEvent;
use crate::publisher::feed_publisher::FeedPublisher;
use crate::source::sources::Sources;
use crate::subscriber::discord_channel_subscriber::DiscordChannelSubscriber;
use crate::subscriber::discord_dm_subscriber::DiscordDmSubscriber;
use dotenv::dotenv;
use log::info;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    env_logger::init();
    info!("Starting pwr-bot...");

    let config = Arc::new(Config::new());
    let event_bus = Arc::new(EventBus::new());

    // Setup database
    let db = Arc::new(Database::new(&config.db_url, &config.db_path).await?);
    db.create_all_tables().await?;
    info!("Database setup complete.");

    // Setup sources
    let sources = Arc::new(Sources::new());

    // Setup & start bot
    let mut bot = Bot::new(config.clone(), db.clone(), sources.clone()).await?;
    bot.start();
    let bot = Arc::new(bot);
    info!("Bot setup complete.");

    // Setup subscribers
    let dm_subscriber = DiscordDmSubscriber::new(bot.clone(), db.clone());
    event_bus.register_subcriber::<SeriesUpdateEvent, _>(dm_subscriber.into());
    let webhook_subscriber =
        DiscordWebhookSubscriber::new(bot.clone(), db.clone(), config.webhook_url.clone());
    event_bus.register_subcriber::<SeriesUpdateEvent, _>(webhook_subscriber.into());
    info!("Subscribers setup complete.");

    // Setup publishers
    FeedPublisher::new(
        db.clone(),
        event_bus.clone(),
        sources.clone(),
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
