use crate::bot::commands::Cog;
use crate::bot::commands::Context;
use crate::bot::commands::Error;

pub mod commands;
pub mod image_generator;
pub mod views;

pub struct VoiceCog;

impl VoiceCog {
    #[poise::command(slash_command, subcommands("Self::settings", "Self::leaderboard"))]
    pub async fn vc(_ctx: Context<'_>) -> Result<(), Error> {
        Ok(())
    }

    #[poise::command(
        slash_command,
        default_member_permissions = "ADMINISTRATOR | MANAGE_GUILD"
    )]
    pub async fn settings(ctx: Context<'_>) -> Result<(), Error> {
        commands::settings(ctx).await
    }

    #[poise::command(slash_command)]
    pub async fn leaderboard(ctx: Context<'_>) -> Result<(), Error> {
        commands::leaderboard(ctx).await
    }
}

impl Cog for VoiceCog {
    fn commands(&self) -> Vec<poise::Command<crate::bot::Data, crate::bot::commands::Error>> {
        vec![Self::vc()]
    }
}
