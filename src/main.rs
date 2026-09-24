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
use pwr_bot::event::VoiceStateEvent;
use pwr_bot::event::event_bus::EventBus;
use pwr_bot::logging::setup_logging;
use pwr_bot::repo::PgRepos;
use pwr_bot::repo::traits::Repos;
use pwr_bot::service::Services;
use pwr_bot::subscriber::voice_state::VoiceStateSubscriber;
use pwr_bot::task::voice_heartbeat::VoiceHeartbeatManager;

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();
    // serenity's rustls_backend links the aws-lc-rs provider via
    // reqwest/hyper-rustls while tokio-postgres-rustls is pinned to ring —
    // both present means rustls 0.23 refuses to auto-pick and panics on the
    // first TLS handshake. Pin ring explicitly (matches the repo pin).
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("no other rustls CryptoProvider installed yet");

    let init_start = Instant::now();
    let config = load_config().await?;
    let event_bus = Arc::new(EventBus::new());

    let repos = setup_database(&config, init_start).await?;
    let services = setup_services(repos.clone()).await?;

    let voice_heartbeat = setup_voice_tracking(&services, init_start).await?;

    let voice_subscriber = Arc::new(VoiceStateSubscriber::new(services.clone()));
    setup_bot(
        &config,
        event_bus.clone(),
        services.clone(),
        repos.clone(),
        voice_subscriber.clone(),
        init_start,
    )
    .await?;

    setup_subscribers(event_bus.clone(), voice_subscriber).await?;

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
        info!("Recovered {recovered} orphaned voice sessions");
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
    services: Arc<Services>,
    repos: Arc<dyn Repos + Send + Sync>,
    voice_subscriber: Arc<VoiceStateSubscriber>,
    init_start: Instant,
) -> Result<Arc<Bot>> {
    info!("Starting bot...");
    let mut bot = Bot::new(config.clone(), event_bus, services, repos, voice_subscriber).await?;

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
    voice_subscriber: Arc<VoiceStateSubscriber>,
) -> Result<()> {
    debug!("Setting up Subscribers...");
    event_bus.register_subcriber::<VoiceStateEvent, _>(voice_subscriber);
    Ok(())
}
