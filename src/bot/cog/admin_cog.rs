/// Cog of owners-only commands
use poise::CreateReply;
use serenity::all::CreateAttachment;

use crate::bot::cog::Context;
use crate::bot::cog::Error;
use crate::database::table::Table;

pub struct AdminCog;

impl AdminCog {
    #[poise::command(prefix_command, owners_only, hide_in_help)]
    pub async fn register(ctx: Context<'_>) -> Result<(), Error> {
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
