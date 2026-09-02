//! Voice settings subcommand.

use std::time::Duration;

use crate::bot::command::prelude::*;
use crate::bot::gui::Host;
use crate::bot::gui::voice_settings::VoiceSettingsConfig;
use crate::bot::gui::voice_settings::VoiceSettingsEffectHandler;
use crate::bot::gui::voice_settings::VoiceSettingsFeature;

/// Configure voice tracking settings for this server
///
/// Enable or disable voice channel activity tracking.
/// Only server administrators can use this command.
#[poise::command(
    slash_command,
    default_member_permissions = "ADMINISTRATOR | MANAGE_GUILD"
)]
pub async fn settings(ctx: Context<'_>) -> Result<(), Error> {
    Router::new(ctx).run(Navigation::SettingsVoice).await?;
    Ok(())
}

handler! { pub struct VoiceSettingsHandler<'a> {} }

#[async_trait::async_trait]
impl CommandHandler for VoiceSettingsHandler<'_> {
    async fn run(&mut self, coordinator: std::sync::Arc<Router<'_>>) -> Result<(), Error> {
        let ctx = *coordinator.context();
        ctx.defer().await?;
        let guild_id = ctx.guild_id().ok_or(BotError::GuildOnlyCommand)?.get();

        let service = ctx.data().service.voice_tracking.clone();

        let settings = service
            .get_server_settings(guild_id)
            .await
            .map_err(Error::from)?;

        let config = VoiceSettingsConfig { guild_id, settings };
        let handler = VoiceSettingsEffectHandler::new(service, guild_id);

        let mut host = Host::<VoiceSettingsFeature, _>::new(
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
