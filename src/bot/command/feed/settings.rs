//! Feed settings subcommand.

use serde_json::json;

use crate::bot::command::prelude::*;
use crate::plugin::command::open_plugin_view;

/// Configure feed settings for this server
///
/// Set up notification channels and required roles for feed subscriptions.
/// Only server administrators can use this command.
#[poise::command(
    slash_command,
    default_member_permissions = "ADMINISTRATOR | MANAGE_GUILD"
)]
pub async fn settings(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(BotError::GuildOnlyCommand)?.get();
    open_plugin_view(
        ctx,
        "feed-settings",
        "feed-settings",
        json!({ "guild_id": guild_id }),
    )
    .await
}
