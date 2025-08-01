use std::sync::Arc;

use anyhow::Result;
use log::error;
use log::info;
use poise::serenity_prelude as serenity;
use ::serenity::all::MessageFlags;
use serenity::all::{CreateMessage, UserId};

use super::Subscriber;
use crate::bot::bot::Bot;
use crate::database::database::Database;
use crate::event::series_update_event::SeriesUpdateEvent;

pub struct DiscordDmSubscriber {
    bot: Arc<Bot>,
    db: Arc<Database>,
}

impl DiscordDmSubscriber {
    pub fn new(bot: Arc<Bot>, db: Arc<Database>) -> Self {
        info!("Initializing DiscordDmSubscriber.");
        Self { bot, db }
    }

    pub async fn series_event_callback(&self, event: SeriesUpdateEvent) -> Result<()> {
        // 1. Create message
        let message = CreateMessage::new().content(format!(
            "🚨 New series update {} -> {} from [{}]({})! 🚨",
            event.previous, event.current, event.title, event.url
        ))
        .flags(MessageFlags::SUPPRESS_EMBEDS);

        // 2. Get all subscribers by latest_results id
        let subscribers = self
            .db
            .subscribers_table
            .select_all_by_latest_results(event.latest_results_id)
            .await?;

        for sub in subscribers {
            let user_id = if let Ok(id) = sub.subscriber_id.parse::<u64>() {
                UserId::new(id)
            } else {
                continue; // Skip invalid IDs, don't return early
            };

            let http = self.bot.http.clone();
            let message = message.clone();

            // Check cache first, but extract the data we need
            let cached_user_exists = self.bot.cache.user(user_id).is_some();

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
                info!(
                    "User {} not in cache, attempting to fetch from HTTP.",
                    user_id
                );
                match http.get_user(user_id).await {
                    Ok(user) => {
                        info!(
                            "Successfully fetched user {}. Attempting to send DM.",
                            user_id
                        );
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
impl Subscriber<SeriesUpdateEvent> for DiscordDmSubscriber {
    async fn callback(&self, event: SeriesUpdateEvent) -> Result<()> {
        let bot = self.bot.clone();
        let db = self.db.clone();

        tokio::spawn(async move {
            let subscriber = DiscordDmSubscriber { bot, db };
            if let Err(e) = subscriber.series_event_callback(event).await {
                error!("Error in spawned DM task: {}", e);
            }
        });

        Ok(())
    }
}
