//! Owner dump_db command.

use poise::CreateReply;
use serenity::all::CreateAttachment;

use crate::bot::commands::Context;
use crate::bot::commands::Error;

#[poise::command(prefix_command, owners_only, hide_in_help)]
pub async fn dump_db(ctx: Context<'_>) -> Result<(), Error> {
    crate::bot::commands::dump_db::command(ctx).await
}

pub async fn command(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer().await?;
    let dump = ctx.data().service.internal.dump_database().await?;

    let reply = CreateReply::default()
        .content("Database dump:")
        .attachment(CreateAttachment::bytes(
            serde_json::to_string_pretty(&dump.feeds)?,
            "feeds.json",
        ))
        .attachment(CreateAttachment::bytes(
            serde_json::to_string_pretty(&dump.feed_items)?,
            "feed_versions.json",
        ))
        .attachment(CreateAttachment::bytes(
            serde_json::to_string_pretty(&dump.subscribers)?,
            "subscribers.json",
        ))
        .attachment(CreateAttachment::bytes(
            serde_json::to_string_pretty(&dump.subscriptions)?,
            "subscriptions.json",
        ));

    ctx.send(reply).await?;
    Ok(())
}
