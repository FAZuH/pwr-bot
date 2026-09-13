//! Welcome commands module.

use serde_json::json;

use crate::bot::command::prelude::*;
use crate::plugin::command::open_plugin_view;

pub mod image_generator;

/// Configure welcome cards for new members
#[poise::command(slash_command)]
pub async fn welcome(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(BotError::GuildOnlyCommand)?.get();
    open_plugin_view(
        ctx,
        "welcome-settings",
        "welcome-settings",
        json!({ "guild_id": guild_id }),
    )
    .await
}
