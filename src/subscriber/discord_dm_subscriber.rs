use std::str::FromStr;
use std::sync::Arc;

use anyhow::Result;
use log::debug;
use log::error;
use log::info;
use poise::serenity_prelude::UserId;
use serenity::all::CreateMessage;

use crate::bot::Bot;
use crate::database::Database;
use crate::database::model::SubscriberModel;
use crate::database::model::SubscriberType;
use crate::event::Event;
use crate::event::FeedUpdateEvent;
use crate::subscriber::Subscriber;

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
        debug!("Received event `{}`", event.event_name());

        // Get all subscriptions for this feed
        let subs = self
            .db
            .subscriber_table
            .select_all_by_type_and_feed(SubscriberType::Dm, event.feed.id)
            .await?;

        for sub in subs {
            if let Err(e) = self.handle_sub(&sub, event.message.clone()).await {
                error!(
                    "Error handling subscriber id `{}` target `{}`: {:?}",
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
