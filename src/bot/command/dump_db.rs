//! Owner-only database inspection command (stub).
//! Plugin data is managed by each plugin independently.

use crate::bot::command::prelude::*;

#[poise::command(prefix_command, owners_only, hide_in_help)]
pub async fn dump_db(ctx: Context<'_>) -> Result<(), Error> {
    ctx.send(
        CreateReply::default()
            .content("Database inspection is handled by individual plugins. Use plugin-specific commands to inspect their data.")
            .ephemeral(true),
    )
    .await?;
    Ok(())
}
