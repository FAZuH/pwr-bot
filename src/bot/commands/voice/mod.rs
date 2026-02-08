use crate::bot::commands::Cog;
use crate::bot::commands::Context;
use crate::bot::commands::Error;
use crate::bot::commands::voice::controller::LeaderboardController;
use crate::bot::commands::voice::controller::SettingsController;

pub mod controller;
pub mod view;
pub mod image_generator;

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
        SettingsController::execute(ctx).await
    }

    #[poise::command(slash_command)]
    pub async fn leaderboard(ctx: Context<'_>) -> Result<(), Error> {
        LeaderboardController::execute(ctx).await
    }
}

impl Cog for VoiceCog {
    fn commands(&self) -> Vec<poise::Command<crate::bot::Data, crate::bot::commands::Error>> {
        vec![Self::vc()]
    }
}
