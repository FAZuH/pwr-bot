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
use pwr_bot::event::FeedUpdateEvent;
use pwr_bot::event::VoiceStateEvent;
use pwr_bot::event::event_bus::EventBus;
use pwr_bot::feed::Platforms;
use pwr_bot::logging::setup_logging;
use pwr_bot::repo::Repository;
use pwr_bot::service::Services;
use pwr_bot::subscriber::discord_dm::DiscordDmSubscriber;
use pwr_bot::subscriber::discord_guild::DiscordGuildSubscriber;
use pwr_bot::subscriber::voice_state::VoiceStateSubscriber;
use pwr_bot::task::series_feed_publisher::SeriesFeedPublisher;
use pwr_bot::task::voice_heartbeat::VoiceHeartbeatManager;

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();

    let init_start = Instant::now();
    let config = load_config().await?;
    let event_bus = Arc::new(EventBus::new());

    let db = setup_database(&config, init_start).await?;
    let platforms = Arc::new(Platforms::new());
    let services = setup_services(db.clone(), platforms.clone()).await?;

    let voice_heartbeat = setup_voice_tracking(&services, init_start).await?;

    let voice_subscriber = Arc::new(VoiceStateSubscriber::new(services.clone()));
    let bot = setup_bot(
        &config,
        event_bus.clone(),
        platforms,
        services.clone(),
        voice_subscriber.clone(),
        init_start,
    )
    .await?;

    setup_subscribers(
        event_bus.clone(),
        bot.clone(),
        services.clone(),
        voice_subscriber,
    )
    .await?;
    setup_publishers(&config, &services, event_bus.clone(), init_start)?;

    info!(
        "pwr-bot is up in {:.2}s. Press Ctrl+C to stop.",
        init_start.elapsed().as_secs_f64()
    );
    tokio::signal::ctrl_c().await?;
    info!("Ctrl+C received, shutting down.");
    voice_heartbeat.update().await;

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

async fn setup_database(config: &Config, init_start: Instant) -> Result<Arc<Repository>> {
    debug!("Setting up Database...");
    let db = Arc::new(Repository::new(&config.db_url).await?);

    info!("Running database migrations...");
    db.run_migrations().await?;
    info!(
        "Database setup complete ({:.2}s).",
        init_start.elapsed().as_secs_f64()
    );

    Ok(db)
}

async fn setup_services(db: Arc<Repository>, platforms: Arc<Platforms>) -> Result<Arc<Services>> {
    debug!("Setting up Services...");
    Ok(Arc::new(Services::new(db, platforms).await?))
}

async fn setup_voice_tracking(
    services: &Services,
    init_start: Instant,
) -> Result<Arc<VoiceHeartbeatManager>> {
    let voice_heartbeat = Arc::new(VoiceHeartbeatManager::new(
        services.internal.clone(),
        services.voice_tracking.clone(),
    ));

    info!("Performing voice tracking crash recovery...");
    let recovered = voice_heartbeat.recover_from_crash().await?;
    if recovered > 0 {
        info!("Recovered {} orphaned voice sessions", recovered);
    }

    voice_heartbeat.clone().start().await;
    debug!(
        "Voice tracking setup complete ({:.2}s).",
        init_start.elapsed().as_secs_f64()
    );

    Ok(voice_heartbeat.clone())
}

async fn setup_bot(
    config: &Arc<Config>,
    event_bus: Arc<EventBus>,
    platforms: Arc<Platforms>,
    services: Arc<Services>,
    voice_subscriber: Arc<VoiceStateSubscriber>,
    init_start: Instant,
) -> Result<Arc<Bot>> {
    info!("Starting bot...");
    let mut bot = Bot::new(
        config.clone(),
        event_bus,
        platforms,
        services,
        voice_subscriber,
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

async fn setup_subscribers(
    event_bus: Arc<EventBus>,
    bot: Arc<Bot>,
    services: Arc<Services>,
    voice_subscriber: Arc<VoiceStateSubscriber>,
) -> Result<()> {
    debug!("Setting up Subscribers...");

    let discord_dm_subscriber = Arc::new(DiscordDmSubscriber::new(bot.clone(), services.clone()));
    let discord_channel_subscriber = Arc::new(DiscordGuildSubscriber::new(bot, services));

    event_bus
        .register_subcriber::<FeedUpdateEvent, _>(discord_dm_subscriber)
        .register_subcriber::<FeedUpdateEvent, _>(discord_channel_subscriber)
        .register_subcriber::<VoiceStateEvent, _>(voice_subscriber);

    Ok(())
}

fn setup_publishers(
    config: &Config,
    services: &Services,
    event_bus: Arc<EventBus>,
    init_start: Instant,
) -> Result<()> {
    if !config.features.feed_publisher {
        return Ok(());
    }
    debug!("Setting up Publishers...");

    SeriesFeedPublisher::new(
        services.feed_subscription.clone(),
        event_bus,
        config.poll_interval,
    )
    .start()?;

    info!(
        "Publishers setup complete ({:.2}s).",
        init_start.elapsed().as_secs_f64()
    );
    Ok(())
}
