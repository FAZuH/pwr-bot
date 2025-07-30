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
use crate::event::anime_update_event::AnimeUpdateEvent;
use crate::event::event_bus::EventBus;
use crate::event::manga_update_event::MangaUpdateEvent;
use crate::publisher::anime_update_publisher::AnimeUpdatePublisher;
use crate::publisher::manga_update_publisher::MangaUpdatePublisher;
use crate::source::ani_list_source::AniListSource;
use crate::source::manga_dex_source::MangaDexSource;
use crate::subscriber::discord_dm_subscriber::DiscordDmSubscriber;
use crate::subscriber::discord_webhook_subscriber::DiscordWebhookSubscriber;
use dotenv::dotenv;
use log::info;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    env_logger::init();
    info!("Bot starting up...");
    let config = Arc::new(Config::new());
    let event_bus = Arc::new(EventBus::new());

    // Setup database
    let db = Arc::new(Database::new(&config.db_url, &config.db_path).await?);
    db.create_all_tables().await?;

    // Setup sources
    let anime_source = Arc::new(AniListSource::new());
    let manga_source = Arc::new(MangaDexSource::new());

    // Setup & start bot
    let mut bot = Bot::new(
        config.clone(),
        db.clone(),
        anime_source.clone(),
        manga_source.clone(),
    )
    .await?;
    bot.start();
    let bot = Arc::new(bot);

    // Setup subscribers
    let dm_subscriber = Arc::new(DiscordDmSubscriber::new(bot.clone(), db.clone()));
    event_bus
        .register_subcriber::<AnimeUpdateEvent, _>(dm_subscriber.clone())
        .await;
    event_bus
        .register_subcriber::<MangaUpdateEvent, _>(dm_subscriber)
        .await;

    let webhook_subscriber = Arc::new(DiscordWebhookSubscriber::new(
        bot.clone(),
        config.webhook_url.clone(),
    ));
    event_bus
        .register_subcriber::<AnimeUpdateEvent, _>(webhook_subscriber.clone())
        .await;
    event_bus
        .register_subcriber::<MangaUpdateEvent, _>(webhook_subscriber)
        .await;

    // Setup publishers
    AnimeUpdatePublisher::new(
        db.clone(),
        event_bus.clone(),
        anime_source.clone(),
        config.poll_interval,
    )
    .start()?;
    MangaUpdatePublisher::new(
        db.clone(),
        event_bus.clone(),
        manga_source.clone(),
        config.poll_interval,
    )
    .start()?;

    // Listen for exit signal
    info!("pwr-bot is up. Press Ctrl+C to stop.");
    tokio::signal::ctrl_c().await?;
    info!("Ctrl+C received, shutting down.");
    // Stop publishers

    Ok(())
}
