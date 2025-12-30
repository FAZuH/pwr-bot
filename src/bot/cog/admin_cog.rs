/// Cog of owners-only commands
use poise::CreateReply;
use poise::samples::create_application_commands;
use serenity::all::CreateAttachment;

use crate::bot::cog::Context;
use crate::bot::cog::Error;
use crate::bot::checks::check_guild_permissions;
use crate::bot::error::BotError;
use crate::database::table::Table;

pub struct AdminCog;

impl AdminCog {
    #[poise::command(prefix_command, hide_in_help)]
    pub async fn register(ctx: Context<'_>) -> Result<(), Error> {
        check_guild_permissions(ctx, &None).await?;
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
        check_guild_permissions(ctx, &None).await?;
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

    #[poise::command(prefix_command, owners_only, hide_in_help)]
    pub async fn register_owner(ctx: Context<'_>) -> Result<(), Error> {
        poise::builtins::register_application_commands_buttons(ctx).await?;
        Ok(())
    }

    #[poise::command(prefix_command, owners_only, hide_in_help)]
    pub async fn dump_db(ctx: Context<'_>) -> Result<(), Error> {
        ctx.defer().await?;
        let db = ctx.data().db.clone();

        let feeds = db.feed_table.select_all().await?;
        let versions = db.feed_item_table.select_all().await?;
        let subscribers = db.subscriber_table.select_all().await?;
        let subscriptions = db.feed_subscription_table.select_all().await?;

        let reply = CreateReply::default()
            .content("Database dump:")
            .attachment(CreateAttachment::bytes(
                serde_json::to_string_pretty(&feeds)?,
                "feeds.json",
            ))
            .attachment(CreateAttachment::bytes(
                serde_json::to_string_pretty(&versions)?,
                "feed_versions.json",
            ))
            .attachment(CreateAttachment::bytes(
                serde_json::to_string_pretty(&subscribers)?,
                "subscribers.json",
            ))
            .attachment(CreateAttachment::bytes(
                serde_json::to_string_pretty(&subscriptions)?,
                "subscriptions.json",
            ));

        ctx.send(reply).await?;
        Ok(())
    }
}
