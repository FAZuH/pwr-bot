use std::sync::Arc;

use anyhow::{self, Result};
use serenity::all::{ExecuteWebhook, Webhook};

use crate::{
    bot::bot::Bot,
    event::{anime_update_event::AnimeUpdateEvent, manga_update_event::MangaUpdateEvent},
    subscriber::subscriber::Subscriber,
};

pub struct DiscordWebhookSubscriber {
    bot: Arc<Bot>,
    webhook: Arc<Webhook>,
}

impl DiscordWebhookSubscriber {
    pub fn new(bot: Arc<Bot>, webhook: Arc<Webhook>) -> Self {
        Self { bot, webhook }
    }

    pub async fn anime_event_callback(&self, event: AnimeUpdateEvent) -> anyhow::Result<()> {
        // 1. Create message
        let message = ExecuteWebhook::new().content(format!(
            "🚨 New episode {} from anime {}! 🚨",
            event.episode, event.title
        ));

        // 2. Notify event to all serenity::User DMs
        self.webhook
            .execute(self.bot.client.http.clone(), false, message)
            .await?;
        Ok(())
    }

    pub async fn manga_event_callback(&self, event: MangaUpdateEvent) -> anyhow::Result<()> {
        // 1. Create message
        let message = ExecuteWebhook::new().content(format!(
            "🚨 New chapter {} from manga {}! 🚨",
            event.chapter, event.title
        ));

        // 2. Notify event to all serenity::User DMs
        self.webhook
            .execute(self.bot.client.http.clone(), false, message)
            .await?;
        Ok(())
    }
}

#[async_trait::async_trait]
impl Subscriber<AnimeUpdateEvent> for DiscordWebhookSubscriber {
    async fn callback(&self, event: AnimeUpdateEvent) -> Result<()> {
        DiscordWebhookSubscriber::new(self.bot.clone(), self.webhook.clone())
            .anime_event_callback(event)
            .await
    }
}

#[async_trait::async_trait]
impl Subscriber<MangaUpdateEvent> for DiscordWebhookSubscriber {
    async fn callback(&self, event: MangaUpdateEvent) -> Result<()> {
        DiscordWebhookSubscriber::new(self.bot.clone(), self.webhook.clone())
            .manga_event_callback(event)
            .await
    }
}
