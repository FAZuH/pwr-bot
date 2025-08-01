use std::sync::Arc;

use anyhow::Result;
use log::debug;
use log::info;
use serenity::all::MessageFlags;
use serenity::all::{ExecuteWebhook, Webhook};

use super::Subscriber;
use crate::bot::bot::Bot;
use crate::event::series_update_event::SeriesUpdateEvent;

pub struct DiscordWebhookSubscriber {
    bot: Arc<Bot>,
    webhook_url: String,
}

impl DiscordWebhookSubscriber {
    pub fn new(bot: Arc<Bot>, webhook_url: String) -> Self {
        info!("Initializing DiscordWebhookSubscriber.");
        Self { bot, webhook_url }
    }

    pub async fn series_event_callback(&self, event: SeriesUpdateEvent) -> anyhow::Result<()> {
        // 1. Create message
        let payload = ExecuteWebhook::new().content(format!(
            "🚨 New series update {} -> {} from [{}]({})! 🚨",
            event.previous, event.current, event.title, event.url
        )).flags(MessageFlags::SUPPRESS_EMBEDS);

        // 2. Notify event to all serenity::User DMs
        debug!(
            "Attempting to execute webhook for series update: {}",
            event.title
        );
        let webhook = Webhook::from_url(self.bot.http.clone(), self.webhook_url.as_str()).await?;
        webhook
            .execute(self.bot.http.clone(), false, payload)
            .await?;
        info!(
            "Successfully executed webhook for series update: {}",
            event.title
        );
        Ok(())
    }
}

#[async_trait::async_trait]
impl Subscriber<SeriesUpdateEvent> for DiscordWebhookSubscriber {
    async fn callback(&self, event: SeriesUpdateEvent) -> Result<()> {
        DiscordWebhookSubscriber::new(self.bot.clone(), self.webhook_url.clone())
            .series_event_callback(event)
            .await
    }
}
