use async_trait::async_trait;
use serenity::{http::Http, model::webhook::Webhook, builder::ExecuteWebhook};
use std::sync::Arc;
use crate::action::action::Action;
use crate::event::manga_update_event::MangaUpdateEvent;

pub struct DiscordWebhookAction {
    pub webhook: Webhook,
    pub webhook_url: String,
    http: Arc<Http>,
}

impl DiscordWebhookAction {
    pub async fn new(webhook_url: String) -> anyhow::Result<Self> {
        let http = Arc::new(Http::new(""));
        let webhook = Webhook::from_url(&http, webhook_url.as_str()).await?;
        Ok(Self { webhook, http, webhook_url })
    }
}

#[async_trait]
impl Action for DiscordWebhookAction {
    async fn run(&self, event: &MangaUpdateEvent) -> anyhow::Result<()> {
        let message = format!(
            "New {} update for **{}**! {} {}: {}",
            event.series_type,
            event.title,
            if event.series_type == "manga" { "Chapter" } else { "Episode" },
            event.chapter,
            event.url
        );
        let builder = ExecuteWebhook::new().content(message);
        self.webhook
            .execute(&self.http, false, builder)
            .await?;
        Ok(())
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
