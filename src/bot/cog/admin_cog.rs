/// Cog of server admin only commands
use poise::CreateReply;
use poise::samples::create_application_commands;

use crate::bot::checks::is_author_guild_admin;
use crate::bot::cog::Context;
use crate::bot::cog::Error;
use crate::bot::error::BotError;

pub struct AdminCog;

impl AdminCog {
    #[poise::command(prefix_command, hide_in_help)]
    pub async fn register(ctx: Context<'_>) -> Result<(), Error> {
        is_author_guild_admin(ctx).await?;
        let guild_id = ctx.guild_id().ok_or(BotError::GuildOnlyCommand)?;

        let create_commands = create_application_commands(&ctx.framework().options().commands);
        let num_commands = create_commands.len();

        let start_time = std::time::Instant::now();
        let reply = ctx
            .reply(format!(
                ":gear: Registering {num_commands} guild commands..."
            ))
            .await?;
        guild_id.set_commands(ctx.http(), &create_commands).await?;

        reply
            .edit(
                ctx,
                CreateReply::default().content(format!(
                    ":white_check_mark: Done! Took {}ms",
                    start_time.elapsed().as_millis()
                )),
            )
            .await?;

        Ok(())
    }

    #[poise::command(prefix_command, hide_in_help)]
    pub async fn unregister(ctx: Context<'_>) -> Result<(), Error> {
        is_author_guild_admin(ctx).await?;
        let guild_id = ctx.guild_id().ok_or(BotError::GuildOnlyCommand)?;

        let start_time = std::time::Instant::now();
        let reply = ctx.reply(":gear: Unregistering guild commands...").await?;
        guild_id.set_commands(ctx.http(), &[]).await?;

        reply
            .edit(
                ctx,
                CreateReply::default().content(format!(
                    ":white_check_mark: Done! Took {}ms",
                    start_time.elapsed().as_millis()
                )),
            )
            .await?;

        Ok(())
    }
}
