use std::sync::Arc;

use anyhow::Result;
use serenity::all::{CreateMessage, UserId};

use crate::{bot::bot::Bot, database::{database::Database, model::latest_updates_model::LatestUpdatesModel}, event::{anime_update_event::AnimeUpdateEvent, manga_update_event::MangaUpdateEvent}};

pub struct DiscordDmSubscriber {
    bot: Arc<Bot>,
    db: Arc<Database>
}

impl DiscordDmSubscriber {
    pub fn new(&self, bot: Arc<Bot>, db: Arc<Database>) -> Self {
        Self { bot, db }
    }

    pub async fn anime_event_callback(&self, event: &AnimeUpdateEvent) -> Result<()> {
        let message = CreateMessage::new()
            .content(format!("🚨 New episode {} from anime {}! 🚨", event.episode, event.title));
        self.common(event.series_type.clone(), event.series_id.clone(), message).await
    }

    pub async fn manga_event_callback(&self, event: &MangaUpdateEvent) -> Result<()> {
        let message = CreateMessage::new()
            .content(format!("🚨 New chapter {} from manga {}! 🚨", event.chapter, event.title));
        self.common(event.series_type.clone(), event.series_id.clone(), message).await
    }

    async fn common(&self, series_type: String, series_id: String, message: CreateMessage) -> Result<()> {
        // 1. Get latest_update model by type and series_id
        let model = LatestUpdatesModel { 
            r#type: series_type,
            series_id: series_id,
            ..Default::default()
        };
        let id = self.db.latest_updates_table.select_by_model(&model).await?.id;

        // 2. Get all subscribers by latest_update.id
        let subscribers = self.db.subscribers_table.select_all_by_type_and_latest_update("dm".to_string(), id).await?;

        for sub in subscribers {
            // 4. Get all serenity::User by subscriber.id
            let user_id = if let Ok(id) = sub.subscriber_id.parse::<u64>() {
                UserId::new(id)
            } else {
                return Ok(())
            };

            // 5. Notify event to all serenity::User DMs
            let http = self.bot.client.http.clone();
            let message = message.clone();
            // 5.1 Try getting user from cache first
            // Note: User from cache and user from http has different type, so can't use Result.map()
            if let Some(user) = self.bot.client.cache.user(user_id)  {
                user.dm(http, message).await?;
            } else {
                // 5.2 If user not in cache, get from http
                let user = self.bot.client.http.get_user(user_id).await?;
                user.dm(http, message).await?;
            }
        }
        Ok(())
    }
}
