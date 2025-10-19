use std::sync::Arc;

use anyhow::Result;
use log::{debug, error, info};
use poise::serenity_prelude as serenity;
use serenity::all::UserId;

use super::Subscriber;
use crate::bot::bot::Bot;
use crate::database::database::Database;
use crate::database::model::SubscriberType;
use crate::event::Event;
use crate::event::feed_update_event::FeedUpdateEvent;
use crate::subscriber::event_message_builder::EventMessageBuilder;

pub struct DiscordDmSubscriber {
    bot: Arc<Bot>,
    db: Arc<Database>,
}

impl DiscordDmSubscriber {
    pub fn new(bot: Arc<Bot>, db: Arc<Database>) -> Self {
        debug!("Initializing DiscordDmSubscriber.");
        Self { bot, db }
    }

    pub async fn feed_event_callback(&self, event: FeedUpdateEvent) -> Result<()> {
        debug!("Received {}: {:?}", event.event_name(), event);

        let message = EventMessageBuilder::new(&event).build();

        // Get all subscriptions for this feed
        let subs = self
            .db
            .subscriber_table
            .select_by_type_and_feed(SubscriberType::Dm, event.feed_id)
            .await?;

        for sub in subs {
            let user_id = match sub.target_id.parse::<u64>() {
                Ok(id) => UserId::new(id),
                Err(e) => {
                    error!("Invalid user ID {}: {}", sub.target_id, e);
                    continue;
                }
            };

            let http = self.bot.http.clone();
            let message = message.clone();

            // Check cache first
            if self.bot.cache.user(user_id).is_some() {
                info!("Sending DM to cached user {}.", user_id);
                if let Err(e) = user_id.dm(&http, message).await {
                    error!("Failed to send DM to cached user {}: {}", user_id, e);
                } else {
                    info!("Successfully sent DM to cached user {}.", user_id);
                }
            } else {
                // User not in cache, fetch from HTTP
                debug!("User {} not in cache, fetching from HTTP.", user_id);
                match http.get_user(user_id).await {
                    Ok(user) => {
                        debug!("Fetched user {}. Sending DM.", user_id);
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
impl Subscriber<FeedUpdateEvent> for DiscordDmSubscriber {
    async fn callback(&self, event: FeedUpdateEvent) -> Result<()> {
        self.feed_event_callback(event).await
    }
}
