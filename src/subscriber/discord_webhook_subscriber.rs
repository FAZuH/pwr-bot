use std::sync::Arc;

use anyhow::{self, Result};
use serenity::all::{ExecuteWebhook, Webhook};

use crate::{bot::bot::Bot, event::{anime_update_event::AnimeUpdateEvent, manga_update_event::MangaUpdateEvent}};

pub struct DiscordWebhookSubscriber {
    bot: Arc<Bot>,
    webhook: Webhook
}

impl DiscordWebhookSubscriber {
    pub async fn new(&self, bot: Arc<Bot>, webhook_url: String) -> Result<Self> {
        Ok(Self {
            webhook: Webhook::from_url(bot.client.http.clone(), webhook_url.as_str()).await?,
            bot,
        })
    }

    pub async fn anime_event_callback(&self, event: &AnimeUpdateEvent) -> anyhow::Result<()> {
        // 1. Create message
        let message = ExecuteWebhook::new()
            .content(format!("🚨 New episode {} from anime {}! 🚨", event.episode, event.title));

        // 2. Notify event to all serenity::User DMs
        self.webhook.execute(self.bot.client.http.clone(), false, message).await?;
        Ok(())
    }

    pub async fn manga_event_callback(&self, event: &MangaUpdateEvent) -> anyhow::Result<()> {
        // 1. Create message
        let message = ExecuteWebhook::new()
            .content(format!("🚨 New chapter {} from manga {}! 🚨", event.chapter, event.title));

        // 2. Notify event to all serenity::User DMs
        self.webhook.execute(self.bot.client.http.clone(), false, message).await?;
        Ok(())
    }
}
