pub mod action;
pub mod bot;
pub mod config;
pub mod database;
pub mod event;
pub mod publisher;
pub mod source;
pub mod subscriber;

use crate::config::Config;
use dotenv::dotenv;
use crate::database::database::Database;
use crate::event::event_bus::EventBus;
use crate::publisher::anime_update_publisher::AnimeUpdatePublisher;
use crate::publisher::manga_update_publisher::MangaUpdatePublisher;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    let config = Config::new();
    let db = Arc::new(Database::new(&config.db_url, &config.db_path).await?);
    let event_bus = Arc::new(EventBus::new());

    db.create_all_tables().await?;

    // Setup publisher
    let mut anime_publisher =
        AnimeUpdatePublisher::new(db.clone(), event_bus.clone(), config.poll_interval.clone()).await?;
    let mut manga_publisher =
        MangaUpdatePublisher::new(db.clone(), event_bus.clone(), config.poll_interval.clone()).await?;

    anime_publisher.start()?;
    manga_publisher.start()?;

    // Setup & start bot
    bot::bot::start(Arc::new(config), db).await;

    // Listen for exit signal
    tokio::signal::ctrl_c().await?;
    Ok(())
}
