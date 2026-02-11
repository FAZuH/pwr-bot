//! Admin commands for server management.

use poise::Command;

use crate::bot::Data;
use crate::bot::commands::Cog;
use crate::bot::commands::Context;
use crate::bot::commands::Error;

pub mod controllers;
pub mod views;

/// Cog of server admin only commands.
pub struct AdminCog;

impl AdminCog {
    /// Opens main server settings.
    ///
    /// Requires server administrator permissions.
    #[poise::command(slash_command)]
    pub async fn settings(ctx: Context<'_>) -> Result<(), Error> {
        controllers::settings(ctx).await
    }

    /// Unregister guild slash commands.
    ///
    /// Removes all bot slash commands from the current server.
    /// Requires server administrator permissions.
    #[poise::command(prefix_command)]
    pub async fn register(ctx: Context<'_>) -> Result<(), Error> {
        controllers::register(ctx).await
    }

    /// TODO
    #[poise::command(prefix_command)]
    pub async fn unregister(ctx: Context<'_>) -> Result<(), Error> {
        controllers::unregister(ctx).await
    }
}

impl Cog for AdminCog {
    fn commands(&self) -> Vec<Command<Data, Error>> {
        vec![Self::register(), Self::unregister(), Self::settings()]
    }
}
