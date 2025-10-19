use std::sync::Arc;

use log::{debug, error, info};
use serenity::all::ChannelId;

use super::Subscriber;
use crate::bot::bot::Bot;
use crate::database::database::Database;
use crate::database::model::SubscriberType;
use crate::database::table::Table;
use crate::event::Event;
use crate::event::feed_update_event::FeedUpdateEvent;
use crate::subscriber::event_message_builder::EventMessageBuilder;
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

        let subs = self
            .db
            .subscriber_table
            .select_by_type_and_feed(SubscriberType::Guild, event.feed_id)
            .await?;

        for sub in subs {
            let channel_id = match sub.target_id.parse::<u64>() {
                Ok(id) => ChannelId::new(id),
                Err(e) => {
                    error!("Invalid channel ID {}: {}", sub.target_id, e);
                    continue;
                }
            };

            let message = EventMessageBuilder::new(&event).build();

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
