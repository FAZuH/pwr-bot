use poise::CreateReply;
use poise::samples::create_application_commands;

use crate::bot::checks::is_author_guild_admin;
use crate::bot::commands::Context;
use crate::bot::commands::Error;
use crate::bot::commands::admin::views::SettingsMainAction;
use crate::bot::commands::admin::views::SettingsMainView;
use crate::bot::error::BotError;
use crate::bot::views::InteractableComponentView;
use crate::bot::views::ResponseComponentView;
use crate::database::model::ServerSettingsModel;
use crate::database::table::Table;

pub async fn settings(ctx: Context<'_>) -> Result<(), Error> {
    is_author_guild_admin(ctx).await?;
    let guild_id = ctx.guild_id().ok_or(BotError::GuildOnlyCommand)?;

    let settings = ctx
        .data()
        .db
        .server_settings_table
        .select(&guild_id.into())
        .await?
        .unwrap_or(ServerSettingsModel {
            guild_id: guild_id.into(),
            ..Default::default()
        });

    let mut view = SettingsMainView::new(&ctx, settings);
    let msg_handle = ctx.send(view.create_reply()).await?;

    while let Some((action, _)) = view.listen_once().await {
        msg_handle.edit(ctx, view.create_reply()).await?;
        if !matches!(action, SettingsMainAction::About) {
            ctx.data()
                .db
                .server_settings_table
                .replace(&view.settings)
                .await?;
        }
    }

    Ok(())
}

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
