//! Subscriber that sends feed updates via Discord DM.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use log::debug;
use log::error;
use log::info;
use serde_json::Value;
use serde_json::json;
use tokio::sync::Mutex;

use crate::entity::SubscriberEntity;
use crate::entity::SubscriberType;
use crate::event::FeedUpdateEvent;
use crate::host_client::HostClient;
use crate::service::feed_subscription::FeedSubscriptionService;
use crate::subscriber::Subscriber;

// ponytail: reset at 1,024 entries; use a true LRU if DM traffic makes the reset measurable.
const MAX_CACHED_DM_CHANNELS: usize = 1_024;

fn cache_channel(channels: &mut HashMap<u64, u64>, user_id: u64, channel_id: u64) {
    channels.insert(user_id, channel_id);
    if channels.len() > MAX_CACHED_DM_CHANNELS {
        channels.clear();
    }
}

/// Subscriber that sends feed updates to users via DM.
pub struct DiscordDmSubscriber {
    service: Arc<FeedSubscriptionService>,
    host: Arc<dyn HostClient>,
    channels: Arc<Mutex<HashMap<u64, u64>>>,
}

impl DiscordDmSubscriber {
    /// Creates a DM subscriber.
    pub fn new(service: Arc<FeedSubscriptionService>, host: Arc<dyn HostClient>) -> Self {
        Self {
            service,
            host,
            channels: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Handles a feed update event by sending DMs to subscribers.
    pub async fn feed_event_callback(&self, event: FeedUpdateEvent) -> Result<()> {
        let subscribers = self
            .service
            .get_subscribers_by_type_and_feed(SubscriberType::Dm, event.feed.id)
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

    /// Sends a message to a subscriber through their cached DM channel.
    pub async fn handle_sub(&self, subscriber: &SubscriberEntity, message: Value) -> Result<()> {
        let user_id = subscriber
            .target_id
            .parse::<u64>()
            .map_err(|error| anyhow::anyhow!("invalid DM user id: {error}"))?;
        let channel_id = self.channel_for_user(user_id).await?;
        let result = self.send_to_channel(channel_id, message).await;
        if result.is_err() {
            self.channels.lock().await.remove(&user_id);
        }
        result?;
        info!("successfully sent feed update DM to user id `{user_id}`");
        Ok(())
    }

    async fn send_to_channel(&self, channel_id: u64, message: Value) -> Result<()> {
        self.host
            .call(
                "host.send_message",
                json!({ "channel_id": channel_id, "data": message }),
            )
            .await?;
        Ok(())
    }

    async fn channel_for_user(&self, user_id: u64) -> Result<u64> {
        if let Some(channel_id) = self.channels.lock().await.get(&user_id).copied() {
            return Ok(channel_id);
        }
        debug!("resolving DM channel for user id `{user_id}`");
        let data = self
            .host
            .call("host.open_dm", json!({ "user_id": user_id }))
            .await?;
        let channel_id = data
            .get("channel_id")
            .and_then(Value::as_u64)
            .ok_or_else(|| anyhow::anyhow!("host.open_dm response has no channel_id"))?;
        let mut channels = self.channels.lock().await;
        cache_channel(&mut channels, user_id, channel_id);
        Ok(channel_id)
    }
}

#[async_trait::async_trait]
impl Subscriber<FeedUpdateEvent> for DiscordDmSubscriber {
    async fn callback(&self, event: FeedUpdateEvent) -> Result<()> {
        self.feed_event_callback(event).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dm_channel_cache_never_exceeds_its_ceiling() {
        let mut channels = HashMap::new();

        for user_id in 0..=MAX_CACHED_DM_CHANNELS as u64 {
            cache_channel(&mut channels, user_id, user_id + 1);
            assert!(channels.len() <= MAX_CACHED_DM_CHANNELS);
        }
    }
}
