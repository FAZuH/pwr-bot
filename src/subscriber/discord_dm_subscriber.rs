use std::sync::Arc;

use anyhow::Result;
use log::{debug, error, info};
use poise::serenity_prelude as serenity;
use serenity::all::{CreateMessage, MessageFlags, UserId};

use super::Subscriber;
use crate::bot::bot::Bot;
use crate::database::database::Database;
use crate::database::model::SubscriberType;
use crate::database::table::Table;
use crate::event::Event;
use crate::event::feed_update_event::FeedUpdateEvent;

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

        let message = CreateMessage::new()
            .content(format!(
                "🚨 **{}** updated: {} → {}\n{}",
                event.title, event.previous_version, event.current_version, event.url
            ))
            .flags(MessageFlags::SUPPRESS_EMBEDS);

        // Get all subscriptions for this feed
        let subscriptions = self
            .db
            .feed_subscription_table
            .select_all_by_feed_id(event.feed_id)
            .await?;

        for subscription in subscriptions {
            // Get subscriber details
            let subscriber = match self
                .db
                .subscriber_table
                .select(&subscription.subscriber_id)
                .await
            {
                Ok(sub) => sub,
                Err(e) => {
                    error!(
                        "Failed to fetch subscriber {}: {}",
                        subscription.subscriber_id, e
                    );
                    continue;
                }
            };

            // Only process DM subscribers
            if !matches!(subscriber.r#type, SubscriberType::Dm) {
                continue;
            }

            let user_id = match subscriber.target_id.parse::<u64>() {
                Ok(id) => UserId::new(id),
                Err(e) => {
                    error!("Invalid user ID {}: {}", subscriber.target_id, e);
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
                info!("User {} not in cache, fetching from HTTP.", user_id);
                match http.get_user(user_id).await {
                    Ok(user) => {
                        info!("Fetched user {}. Sending DM.", user_id);
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
