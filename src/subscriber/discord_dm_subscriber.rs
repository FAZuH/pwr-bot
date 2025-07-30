use std::sync::Arc;

use anyhow::Result;
use serenity::all::{CreateMessage, UserId};
use log::{info, error};

use crate::{
    bot::bot::Bot,
    database::{database::Database, model::latest_updates_model::LatestUpdatesModel},
    event::{anime_update_event::AnimeUpdateEvent, manga_update_event::MangaUpdateEvent},
    subscriber::subscriber::Subscriber,
};

pub struct DiscordDmSubscriber {
    bot: Arc<Bot>,
    db: Arc<Database>,
}

impl DiscordDmSubscriber {
    pub fn new(bot: Arc<Bot>, db: Arc<Database>) -> Self {
        info!("Initializing DiscordDmSubscriber.");
        Self { bot, db }
    }

    pub async fn anime_event_callback(&self, event: AnimeUpdateEvent) -> Result<()> {
        let message = CreateMessage::new().content(format!(
            "🚨 New episode {} from anime {}! 🚨",
            event.episode, event.title
        ));
        self.common(event.series_type.clone(), event.series_id.clone(), message)
            .await
    }

    pub async fn manga_event_callback(&self, event: MangaUpdateEvent) -> Result<()> {
        let message = CreateMessage::new().content(format!(
            "🚨 New chapter {} from manga {}! 🚨",
            event.chapter, event.title
        ));
        self.common(event.series_type.clone(), event.series_id.clone(), message)
            .await
    }

    async fn common(
        &self,
        series_type: String,
        series_id: String,
        message: CreateMessage,
    ) -> Result<()> {
        // 1. Get latest_update model by type and series_id
        let model = LatestUpdatesModel {
            r#type: series_type,
            series_id: series_id,
            ..Default::default()
        };
        let id = self
            .db
            .latest_updates_table
            .select_by_model(&model)
            .await?
            .id;

        // 2. Get all subscribers by latest_update.id
        let subscribers = self
            .db
            .subscribers_table
            .select_all_by_type_and_latest_update("dm".to_string(), id)
            .await?;

        for sub in subscribers {
            let user_id = if let Ok(id) = sub.subscriber_id.parse::<u64>() {
                UserId::new(id)
            } else {
                continue; // Skip invalid IDs, don't return early
            };

            let http = self.bot.client().await?.http.clone();
            let message = message.clone();

            // Check cache first, but extract the data we need
            let cached_user_exists = self.bot.client().await?.cache.user(user_id).is_some();

            if cached_user_exists {
                // User exists in cache, send DM directly using user_id
                info!("Attempting to send DM to cached user {}.", user_id);
                if let Err(e) = user_id.dm(&http, message).await {
                    error!("Failed to send DM to cached user {}: {}", user_id, e);
                } else {
                    info!("Successfully sent DM to cached user {}.", user_id);
                }
            } else {
                // User not in cache, fetch from HTTP then send
                info!("User {} not in cache, attempting to fetch from HTTP.", user_id);
                match self.bot.client().await?.http.get_user(user_id).await {
                    Ok(user) => {
                        info!("Successfully fetched user {}. Attempting to send DM.", user_id);
                        if let Err(e) = user.dm(&http, message).await {
                            error!("Failed to send DM to fetched user {}: {}", user_id, e);
                        } else {
                            info!("Successfully sent DM to fetched user {}.", user_id);
                        }
                    }
                    Err(e) => {
                        error!("Failed to fetch user {}: {}", user_id, e);
                    }
                }
            }
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl Subscriber<AnimeUpdateEvent> for DiscordDmSubscriber {
    async fn callback(&self, event: AnimeUpdateEvent) -> Result<()> {
        DiscordDmSubscriber::new(self.bot.clone(), self.db.clone())
            .anime_event_callback(event)
            .await
    }
}

#[async_trait::async_trait]
impl Subscriber<MangaUpdateEvent> for DiscordDmSubscriber {
    async fn callback(&self, event: MangaUpdateEvent) -> Result<()> {
        DiscordDmSubscriber::new(self.bot.clone(), self.db.clone())
            .manga_event_callback(event)
            .await
    }
}
