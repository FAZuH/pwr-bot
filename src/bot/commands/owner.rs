//! Owner-only commands for bot administration.

use poise::Command;
use poise::CreateReply;
use serenity::all::CreateAttachment;

use crate::bot::Data;
use crate::bot::commands::Cog;
use crate::bot::commands::Context;
use crate::bot::commands::Error;

/// Cog of bot owners-only commands.
pub struct OwnerCog;

impl OwnerCog {
    /// Register application commands (owner only)
    ///
    /// Opens a dialog to register global or guild application commands.
    /// Restricted to bot owners only.
    #[poise::command(prefix_command, owners_only, hide_in_help)]
    pub async fn register_owner(ctx: Context<'_>) -> Result<(), Error> {
        poise::builtins::register_application_commands_buttons(ctx).await?;
        Ok(())
    }

    /// Export database contents (owner only)
    ///
    /// Dumps all database tables as JSON files for inspection.
    /// Includes feeds, feed items, subscribers, and subscriptions.
    /// Restricted to bot owners only.
    #[poise::command(prefix_command, owners_only, hide_in_help)]
    pub async fn dump_db(ctx: Context<'_>) -> Result<(), Error> {
        ctx.defer().await?;
        let dump = ctx.data().service.maintenance.dump_database().await?;

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
}

impl Cog for OwnerCog {
    fn commands(&self) -> Vec<Command<Data, Error>> {
        vec![Self::dump_db(), Self::register_owner()]
    }
}
