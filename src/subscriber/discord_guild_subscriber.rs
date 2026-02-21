//! Subscriber that sends feed updates to Discord guild channels.

use std::str::FromStr;
use std::sync::Arc;

use anyhow::Result;
use log::debug;
use log::error;
use log::info;
use serenity::all::ChannelId;
use serenity::all::CreateMessage;
use serenity::all::GuildId;

use crate::bot::Bot;
use crate::entity::SubscriberEntity;
use crate::entity::SubscriberType;
use crate::event::Event;
use crate::event::FeedUpdateEvent;
use crate::repository::Repository;
use crate::repository::table::Table;
use crate::subscriber::Subscriber;

/// Subscriber that sends feed updates to guild channels.
pub struct DiscordGuildSubscriber {
    bot: Arc<Bot>,
    db: Arc<Repository>,
}

impl DiscordGuildSubscriber {
    /// Creates a new guild subscriber.
    pub fn new(bot: Arc<Bot>, db: Arc<Repository>) -> Self {
        debug!("Initializing DiscordGuildSubscriber.");
        Self { bot, db }
    }

    /// Handles a feed update event by sending messages to guild channels.
    pub async fn feed_event_callback(&self, event: FeedUpdateEvent) -> Result<()> {
        debug!("Received event `{}`", event.event_name());

        let subs = self
            .db
            .subscriber
            .select_all_by_type_and_feed(SubscriberType::Guild, event.feed.id)
            .await?;

        for sub in subs {
            if let Err(e) = self.handle_sub(&sub, event.data.create_message()).await {
                error!(
                    "Error handling subscriber id `{}` target `{}`: {:?}",
                    sub.id, sub.target_id, e
                );
            }
        }

        Ok(())
    }

    /// Sends a message to a guild channel for a subscriber.
    pub async fn handle_sub(
        &self,
        sub: &SubscriberEntity,
        message: CreateMessage<'_>,
    ) -> anyhow::Result<()> {
        let guild_id = GuildId::from_str(&sub.target_id)?;

        let settings = self
            .db
            .server_settings
            .select(&guild_id.get())
            .await?
            .ok_or_else(|| anyhow::anyhow!("Settings not found"))?;
        let channel_id_str =
            settings.settings.0.feeds.channel_id.ok_or_else(|| {
                anyhow::anyhow!("No channel configured for guild {}", &sub.target_id)
            })?;

        let channel_id = ChannelId::from_str(&channel_id_str)?;

        debug!("Fetching channel id `{}`.", channel_id);
        let channel = channel_id
            .to_guild_channel(&self.bot.http, Some(guild_id))
            .await?;

        debug!(
            "Fetched channel id `{}` ({}). Sending message.",
            channel_id, channel.base.name
        );
        channel.send_message(&self.bot.http, message).await?;

        info!(
            "Successfully sent message to fetched channel id `{}` ({}).",
            channel_id, channel.base.name
        );
        Ok(())
    }
}

#[async_trait::async_trait]
impl Subscriber<FeedUpdateEvent> for DiscordGuildSubscriber {
    async fn callback(&self, event: FeedUpdateEvent) -> Result<()> {
        self.feed_event_callback(event).await
    }
}
