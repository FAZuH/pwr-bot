//! Application entry point for pwr-bot.
//!
//! Initializes all components and starts the Discord bot.

pub mod bot;
pub mod config;
pub mod database;
pub mod error;
pub mod event;
pub mod feed;
pub mod logging;
pub mod macros;
pub mod service;
pub mod subscriber;
pub mod task;

use std::sync::Arc;
use std::time::Instant;

use dotenv::dotenv;
use log::debug;
use log::info;

use crate::bot::Bot;
use crate::config::Config;
use crate::database::Database;
use crate::event::FeedUpdateEvent;
use crate::event::VoiceStateEvent;
use crate::event::event_bus::EventBus;
use crate::feed::platforms::Platforms;
use crate::logging::setup_logging;
use crate::service::Services;
use crate::subscriber::discord_dm_subscriber::DiscordDmSubscriber;
use crate::subscriber::discord_guild_subscriber::DiscordGuildSubscriber;
use crate::subscriber::voice_state_subscriber::VoiceStateSubscriber;
use crate::task::series_feed_publisher::SeriesFeedPublisher;
use crate::task::voice_heartbeat::VoiceHeartbeatManager;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let init_start = Instant::now();
    dotenv().ok();

    let mut config = Config::new();
    config.load()?;
    let config = Arc::new(config);

    setup_logging(&config)?;

    info!("Starting pwr-bot...");

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

    // Setup Platforms
    debug!("Setting up Platforms...");
    let platforms = Arc::new(Platforms::new());

    // Setup Services
    debug!("Setting up Services...");
    let services = Arc::new(Services::new(db.clone(), platforms.clone()).await?);

    // Perform voice tracking crash recovery before starting the bot
    let voice_heartbeat =
        VoiceHeartbeatManager::new(&config.data_path, services.voice_tracking.clone());
    info!("Performing voice tracking crash recovery...");
    let recovered = voice_heartbeat.recover_from_crash().await?;
    if recovered > 0 {
        info!("Recovered {} orphaned voice sessions", recovered);
    }
    voice_heartbeat.start().await;

    // Create voice_subscriber first (needed by BotEventHandler)
    let voice_subscriber = Arc::new(VoiceStateSubscriber::new(services.clone()));

    // Setup & start bot (needs voice_subscriber for voice channel scanning)
    info!("Starting bot...");
    let mut bot = Bot::new(
        config.clone(),
        db.clone(),
        event_bus.clone(),
        platforms.clone(),
        services.clone(),
        voice_subscriber.clone(),
    )
    .await?;
    bot.start();
    let bot = Arc::new(bot);
    info!(
        "Bot setup complete ({:.2}s).",
        init_start.elapsed().as_secs_f64()
    );

    // Setup subscribers
    let discord_dm_subscriber = Arc::new(DiscordDmSubscriber::new(bot.clone(), db.clone()));
    let discord_channel_subscriber = Arc::new(DiscordGuildSubscriber::new(bot.clone(), db.clone()));

    debug!("Setting up Subscribers...");
    event_bus
        .register_subcriber::<FeedUpdateEvent, _>(discord_dm_subscriber.clone())
        .register_subcriber::<FeedUpdateEvent, _>(discord_channel_subscriber.clone())
        .register_subcriber::<VoiceStateEvent, _>(voice_subscriber.clone());
    info!(
        "Subscribers setup complete ({:.2}s).",
        init_start.elapsed().as_secs_f64()
    );

    // // Setup publishers
    // debug!("Setting up Publishers...");
    // SeriesFeedPublisher::new(
    //     services.feed_subscription.clone(),
    //     event_bus.clone(),
    //     config.poll_interval,
    // )
    // .start()?;
    // info!(
    //     "Publishers setup complete ({:.2}s).",
    //     init_start.elapsed().as_secs_f64()
    // );

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
