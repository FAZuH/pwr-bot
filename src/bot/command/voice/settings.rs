//! Voice settings subcommand.

use serde_json::json;

use crate::bot::command::prelude::*;
use crate::plugin::command::open_plugin_view;

/// Configure voice tracking settings for this server
///
/// Enable or disable voice channel activity tracking.
/// Only server administrators can use this command.
#[poise::command(
    slash_command,
    default_member_permissions = "ADMINISTRATOR | MANAGE_GUILD"
)]
pub async fn settings(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(BotError::GuildOnlyCommand)?.get();
    open_plugin_view(
        ctx,
        "voice-settings",
        "voice-settings",
        json!({ "guild_id": guild_id }),
    )
    .await
}
