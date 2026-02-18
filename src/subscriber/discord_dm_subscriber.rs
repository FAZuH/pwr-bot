//! Subscriber that sends feed updates via Discord DM.

use std::str::FromStr;
use std::sync::Arc;

use anyhow::Result;
use log::debug;
use log::error;
use log::info;
use poise::serenity_prelude::UserId;
use serenity::all::CreateMessage;

use crate::bot::Bot;
use crate::event::Event;
use crate::event::FeedUpdateEvent;
use crate::model::SubscriberModel;
use crate::model::SubscriberType;
use crate::repository::Repository;
use crate::subscriber::Subscriber;

/// Subscriber that sends feed updates to users via DM.
pub struct DiscordDmSubscriber {
    bot: Arc<Bot>,
    db: Arc<Repository>,
}

impl DiscordDmSubscriber {
    /// Creates a new DM subscriber.
    pub fn new(bot: Arc<Bot>, db: Arc<Repository>) -> Self {
        debug!("Initializing DiscordDmSubscriber.");
        Self { bot, db }
    }

    /// Handles a feed update event by sending DMs to subscribers.
    pub async fn feed_event_callback(&self, event: FeedUpdateEvent) -> Result<()> {
        debug!("Received event `{}`", event.event_name());

        // Get all subscriptions for this feed
        let subs = self
            .db
            .subscriber
            .select_all_by_type_and_feed(SubscriberType::Dm, event.feed.id)
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

    /// Sends a message to a subscriber via DM.
    pub async fn handle_sub(
        &self,
        sub: &SubscriberModel,
        message: CreateMessage<'_>,
    ) -> anyhow::Result<()> {
        let user_id = UserId::from_str(&sub.target_id)?;

        debug!("Fetching user id `{}`.", user_id);
        let user = self.bot.http.get_user(user_id).await?;

        debug!("Fetched user id `{}` ({}). Sending DM.", user_id, user.name);
        user.id.dm(&self.bot.http, message).await?;

        info!(
            "Successfully sent DM to fetched user id `{}` ({}).",
            user_id, user.name
        );
        Ok(())
    }
}

#[async_trait::async_trait]
impl Subscriber<FeedUpdateEvent> for DiscordDmSubscriber {
    async fn callback(&self, event: FeedUpdateEvent) -> Result<()> {
        self.feed_event_callback(event).await
    }
}
