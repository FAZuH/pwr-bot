//! Welcome commands module.
use std::sync::Arc;
use std::time::Duration;

use crate::bot::command::prelude::*;
use crate::bot::command::welcome::image_generator::WelcomeImageGenerator;
use crate::bot::gui::Host;
use crate::bot::gui::welcome::WelcomeConfig;
use crate::bot::gui::welcome::WelcomeFeature;
use crate::bot::gui::welcome::WelcomeSettingsEffectHandler;
use crate::bot::gui::welcome::generate_preview_from;

pub mod image_generator;

/// Configure welcome cards for new members
#[poise::command(slash_command)]
pub async fn welcome(ctx: Context<'_>) -> Result<(), Error> {
    Router::new(ctx).run(Navigation::SettingsWelcome).await?;
    Ok(())
}

// ── Handler ───────────────────────────────────────────────────────────────

handler! { pub struct WelcomeSettingsHandler<'a> {} }

#[async_trait::async_trait]
impl CommandHandler for WelcomeSettingsHandler<'_> {
    async fn run(&mut self, coordinator: std::sync::Arc<Router<'_>>) -> Result<(), Error> {
        let ctx = *coordinator.context();
        ctx.defer().await?;

        let guild_id = ctx.guild_id().ok_or(BotError::GuildOnlyCommand)?.get();
        let service = ctx.data().service.feed_subscription.clone();
        let generator = Arc::new(WelcomeImageGenerator::new());

        let settings = service
            .get_server_settings(guild_id)
            .await
            .map_err(Error::from)?;

        // Boot-load: the initial preview is generated once here, mirroring
        // today's initial-generation timing. Only in-session regeneration is
        // an effect (`RenderImage` -> `ImageRendered`).
        let image_bytes = generate_preview_from(&settings, &generator).await;

        let config = WelcomeConfig {
            settings,
            image_bytes,
        };
        let handler = WelcomeSettingsEffectHandler::new(service, guild_id, generator);

        let mut host = Host::<WelcomeFeature, _>::new(
            ctx,
            config,
            handler,
            Duration::from_secs(120),
            coordinator.clone(),
        );

        host.run().await?;

        Ok(())
    }
}
