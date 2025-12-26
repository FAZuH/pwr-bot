use std::str::FromStr;
use std::sync::Arc;

use anyhow::Result;
use log::debug;
use log::error;
use log::info;
use serenity::all::ChannelId;
use serenity::all::CreateMessage;

use super::Subscriber;
use crate::bot::bot::Bot;
use crate::database::database::Database;
use crate::database::model::SubscriberModel;
use crate::database::model::SubscriberType;
use crate::event::Event;
use crate::event::feed_update_event::FeedUpdateEvent;

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
            .select_by_type_and_feed(SubscriberType::Guild, event.feed.id)
            .await?;

        for sub in subs {
            if let Err(e) = self.handle_sub(&sub, event.message.clone()).await {
                error!(
                    "Error handling user id `{}` target `{}`: {:?}",
                    sub.id, sub.target_id, e
                );
            }
        }

        Ok(())
    }

    pub async fn handle_sub(
        &self,
        sub: &SubscriberModel,
        message: CreateMessage<'_>,
    ) -> anyhow::Result<()> {
        let channel_id = ChannelId::from_str(&sub.target_id)?;

        debug!("Fetching channel id `{}`.", channel_id);
        let channel = channel_id.to_guild_channel(&self.bot.http, None).await?;

        debug!(
            "Fetched channel id `{}` ({}). Sending message.",
            channel_id, channel.base.name
        );
        channel.send_message(&self.bot.http, message).await?;

        info!(
            "Successfully sent DM to fetched user id `{}` ({}).",
            channel_id, channel.base.name
        );
        Ok(())
    }
}

#[async_trait::async_trait]
impl Subscriber<FeedUpdateEvent> for DiscordChannelSubscriber {
    async fn callback(&self, event: FeedUpdateEvent) -> Result<()> {
        self.feed_event_callback(event).await
    }
}
