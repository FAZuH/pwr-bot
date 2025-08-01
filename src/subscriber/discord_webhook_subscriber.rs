use std::sync::Arc;

use anyhow::Result;
use log::debug;
use log::error;
use log::info;
use serenity::all::MessageFlags;
use serenity::all::{ExecuteWebhook, Webhook};

use super::Subscriber;
use crate::bot::bot::Bot;
use crate::database::database::Database;
use crate::event::series_update_event::SeriesUpdateEvent;

pub struct DiscordWebhookSubscriber {
    bot: Arc<Bot>,
    db: Arc<Database>,
    webhook_url: String,
}

impl DiscordWebhookSubscriber {
    pub fn new(bot: Arc<Bot>, db: Arc<Database>, webhook_url: String) -> Self {
        debug!("Initializing DiscordWebhookSubscriber.");
        Self {
            bot,
            db,
            webhook_url,
        }
    }

    pub async fn series_event_callback(&self, event: SeriesUpdateEvent) -> anyhow::Result<()> {
        // 1. Create message
        let payload = ExecuteWebhook::new()
            .content(format!(
                "🚨 New series update {} -> {} from [{}]({})! 🚨",
                event.previous, event.current, event.title, event.url
            ))
            .flags(MessageFlags::SUPPRESS_EMBEDS);

        // 2. Get all subscribers by latest_results id
        let subscribers = self
            .db
            .subscribers_table
            .select_all_by_type_and_latest_results("webhook", event.latest_results_id)
            .await?;

        for sub in subscribers {
            // 2. Notify event to all serenity::User DMs
            debug!(
                "Attempting to execute to webhook {} for series update: {}",
                sub.subscriber_id, event.title
            );
            let webhook = match Webhook::from_url(self.bot.http.clone(), &sub.subscriber_id).await {
                Ok(webhook) => webhook,
                Err(e) => {
                    error!(
                        "Failed to create webhook from URL {}: {}",
                        sub.subscriber_id, e
                    );
                    continue; // Skip this subscriber if webhook creation fails
                }
            };
            webhook
                .execute(self.bot.http.clone(), false, payload.clone())
                .await?;
            info!(
                "Successfully executed webhook for series update: {}",
                event.title
            );
        }

        Ok(())
    }
}

#[async_trait::async_trait]
impl Subscriber<SeriesUpdateEvent> for DiscordWebhookSubscriber {
    async fn callback(&self, event: SeriesUpdateEvent) -> Result<()> {
        DiscordWebhookSubscriber::new(self.bot.clone(), self.db.clone(), self.webhook_url.clone())
            .series_event_callback(event)
            .await
    }
}
