use std::sync::Arc;

use log::{debug, error, info};
use serenity::all::{ChannelId, CreateMessage};

use super::Subscriber;
use crate::bot::bot::Bot;
use crate::database::database::Database;
use crate::database::model::SubscriberType;
use crate::database::table::Table;
use crate::event::Event;
use crate::event::feed_update_event::FeedUpdateEvent;
use anyhow::Result;

pub struct DiscordChannelSubscriber {
    bot: Arc<Bot>,
    db: Arc<Database>,
}

impl DiscordChannelSubscriber {
    pub fn new(bot: Arc<Bot>, db: Arc<Database>) -> Self {
        debug!("Initializing DiscordChannelSubscriber.");
        Self { bot, db }
    }

    pub async fn feed_event_callback(&self, event: FeedUpdateEvent) -> Result<()> {
        debug!("Received {}: {:?}", event.event_name(), event);

        // Get all subscriptions for this feed
        let subscriptions = self
            .db
            .feed_subscription_table
            .select_all_by_feed_id(event.feed_id)
            .await?;

        for subscription in subscriptions {
            // Get subscriber details
            let subscriber = self
                .db
                .subscriber_table
                .select(&subscription.subscriber_id)
                .await?;

            // Only process guild channel subscribers
            if !matches!(subscriber.r#type, SubscriberType::Guild) {
                continue;
            }

            let channel_id = match subscriber.target_id.parse::<u64>() {
                Ok(id) => ChannelId::new(id),
                Err(e) => {
                    error!("Invalid channel ID {}: {}", subscriber.target_id, e);
                    continue;
                }
            };

            let message = CreateMessage::new().content(format!(
                "🚨 **{}** updated: {} → {}\n{}",
                event.title, event.previous_version, event.current_version, event.url
            ));

            debug!(
                "Sending notification to channel {} for feed: {}",
                channel_id, event.title
            );

            match channel_id.send_message(&self.bot.http, message).await {
                Ok(_) => {
                    info!(
                        "Successfully sent notification to channel {} for feed: {}",
                        channel_id, event.title
                    );
                }
                Err(e) => {
                    error!("Failed to send message to channel {}: {}", channel_id, e);
                }
            }
        }

        Ok(())
    }
}

#[async_trait::async_trait]
impl Subscriber<FeedUpdateEvent> for DiscordChannelSubscriber {
    async fn callback(&self, event: FeedUpdateEvent) -> Result<()> {
        self.feed_event_callback(event).await
    }
}
