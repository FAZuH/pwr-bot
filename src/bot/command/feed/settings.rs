//! Feed settings subcommand.

use std::time::Duration;

use crate::bot::command::prelude::*;
use crate::bot::gui::Host;
use crate::bot::gui::feed_settings::FeedSettingsConfig;
use crate::bot::gui::feed_settings::FeedSettingsEffectHandler;
use crate::bot::gui::feed_settings::FeedSettingsFeature;

/// Configure feed settings for this server
///
/// Set up notification channels and required roles for feed subscriptions.
/// Only server administrators can use this command.
#[poise::command(
    slash_command,
    default_member_permissions = "ADMINISTRATOR | MANAGE_GUILD"
)]
pub async fn settings(ctx: Context<'_>) -> Result<(), Error> {
    Router::new(ctx).run(Navigation::SettingsFeeds).await?;
    Ok(())
}

handler! { pub struct FeedSettingsHandler<'a> {} }

#[async_trait::async_trait]
impl CommandHandler for FeedSettingsHandler<'_> {
    async fn run(&mut self, coordinator: std::sync::Arc<Router<'_>>) -> Result<(), Error> {
        let ctx = *coordinator.context();
        ctx.defer().await?;
        let guild_id = ctx.guild_id().ok_or(BotError::GuildOnlyCommand)?.get();

        let service = ctx.data().service.feed_subscription.clone();

        let settings = service
            .get_server_settings(guild_id)
            .await
            .map_err(Error::from)?;

        let config = FeedSettingsConfig { settings };
        let handler = FeedSettingsEffectHandler::new(service, guild_id);

        let mut host = Host::<FeedSettingsFeature, _>::new(
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
