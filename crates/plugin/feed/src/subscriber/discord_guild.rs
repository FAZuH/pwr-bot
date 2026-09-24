//! Subscriber that sends feed updates to Discord guild channels.

use std::sync::Arc;

use anyhow::Result;
use log::debug;
use log::error;
use log::info;
use serde_json::Value;
use serde_json::json;

use crate::entity::SubscriberEntity;
use crate::entity::SubscriberType;
use crate::event::FeedUpdateEvent;
use crate::host_client::HostClient;
use crate::service::feed_subscription::FeedSubscriptionService;
use crate::subscriber::Subscriber;

/// Subscriber that sends feed updates to guild channels.
pub struct DiscordGuildSubscriber {
    service: Arc<FeedSubscriptionService>,
    host: Arc<dyn HostClient>,
}

impl DiscordGuildSubscriber {
    /// Creates a guild subscriber.
    pub fn new(service: Arc<FeedSubscriptionService>, host: Arc<dyn HostClient>) -> Self {
        Self { service, host }
    }

    /// Handles a feed update event by sending messages to guild channels.
    pub async fn feed_event_callback(&self, event: FeedUpdateEvent) -> Result<()> {
        let subscribers = self
            .service
            .get_subscribers_by_type_and_feed(SubscriberType::Guild, event.feed.id)
            .await?;
        let message = event.data.create_message();
        for subscriber in subscribers {
            if let Err(error) = self.handle_sub(&subscriber, message.clone()).await {
                error!(
                    "error handling subscriber id `{}` target `{}`: {error}",
                    subscriber.id, subscriber.target_id
                );
            }
        }
        Ok(())
    }

    /// Sends a message to the configured feed channel for a subscriber.
    pub async fn handle_sub(&self, subscriber: &SubscriberEntity, message: Value) -> Result<()> {
        let guild_id = subscriber
            .target_id
            .parse::<u64>()
            .map_err(|error| anyhow::anyhow!("invalid guild id: {error}"))?;
        let settings = self.service.get_feed_settings(guild_id).await?;
        let channel_id = settings
            .channel_id
            .ok_or_else(|| anyhow::anyhow!("no feed channel configured for guild {guild_id}"))?
            .parse::<u64>()
            .map_err(|error| anyhow::anyhow!("invalid feed channel id: {error}"))?;
        debug!("sending feed update to guild channel id `{channel_id}`");
        self.host
            .call(
                "host.send_message",
                json!({ "channel_id": channel_id, "data": message }),
            )
            .await?;
        info!("successfully sent feed update to guild channel id `{channel_id}`");
        Ok(())
    }
}

#[async_trait::async_trait]
impl Subscriber<FeedUpdateEvent> for DiscordGuildSubscriber {
    async fn callback(&self, event: FeedUpdateEvent) -> Result<()> {
        self.feed_event_callback(event).await
    }
}
