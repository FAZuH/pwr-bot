use crate::bot::cog::Context;
use crate::bot::cog::Error;

pub struct VoiceCog;

impl VoiceCog {
    #[poise::command(
        slash_command,
        guild_only,
        default_member_permissions = "ADMINISTRATOR | MANAGE_GUILD"
    )]
    pub async fn settings(ctx: Context<'_>) -> Result<(), Error> {
        todo!()
    }

    #[poise::command(slash_command)]
    pub async fn leaderboard(ctx: Context<'_>) -> Result<(), Error> {
        todo!()
    }

    #[poise::command(slash_command)]
    pub async fn history(ctx: Context<'_>) -> Result<(), Error> {
        todo!()
    }
}
