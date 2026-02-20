//! Owner register command.

use crate::bot::commands::Context;
use crate::bot::commands::Error;

#[poise::command(prefix_command, owners_only, hide_in_help)]
pub async fn register_owner(ctx: Context<'_>) -> Result<(), Error> {
    command(ctx).await
}

pub async fn command(ctx: Context<'_>) -> Result<(), Error> {
    poise::builtins::register_application_commands_buttons(ctx).await?;
    Ok(())
}
