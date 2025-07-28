pub mod action;
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
use crate::subscriber::discord_dm_subscriber::DiscordDmSubscriber;
use crate::subscriber::discord_webhook_subscriber::DiscordWebhookSubscriber;
use dotenv::dotenv;
use serenity::all::Webhook;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    let shared_config = Arc::new(Config::new());
    let shared_event_bus = Arc::new(EventBus::new());

    // Setup database
    let shared_db = Arc::new(Database::new(&shared_config.db_url, &shared_config.db_path).await?);
    shared_db.create_all_tables().await?;

    // Setup publishers
    AnimeUpdatePublisher::new(
        shared_db.clone(),
        shared_event_bus.clone(),
        shared_config.poll_interval,
    )
    .start()?;
    MangaUpdatePublisher::new(
        shared_db.clone(),
        shared_event_bus.clone(),
        shared_config.poll_interval,
    )
    .start()?;

    // Setup & start bot
    let mut bot = Bot::new(shared_config.clone(), shared_db.clone()).await?;
    bot.start().await?;
    let shared_bot = Arc::new(bot);

    // Setup subscribers
    let dm_subscriber = Arc::new(DiscordDmSubscriber::new(
        shared_bot.clone(),
        shared_db.clone(),
    ));
    shared_event_bus
        .register_subcriber::<AnimeUpdateEvent, _>(dm_subscriber.clone())
        .await;
    shared_event_bus
        .register_subcriber::<MangaUpdateEvent, _>(dm_subscriber)
        .await;

    let webhook_subscriber = Arc::new(DiscordWebhookSubscriber::new(
        shared_bot.clone(),
        Arc::new(
            Webhook::from_url(
                shared_bot.client.http.clone(),
                shared_config.webhook_url.clone().as_str(),
            )
            .await?,
        ),
    ));
    shared_event_bus
        .register_subcriber::<AnimeUpdateEvent, _>(webhook_subscriber.clone())
        .await;
    shared_event_bus
        .register_subcriber::<MangaUpdateEvent, _>(webhook_subscriber)
        .await;

    // Listen for exit signal
    tokio::signal::ctrl_c().await?;
    Ok(())
}
