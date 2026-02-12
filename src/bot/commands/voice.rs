//! Voice channel tracking and leaderboard commands.

use crate::bot::commands::Cog;
use crate::bot::commands::Context;
use crate::bot::commands::Error;
use crate::database::model::VoiceLeaderboardEntry;

pub mod controllers;
pub mod image_generator;
pub mod views;

/// A single entry in the voice leaderboard.
pub struct LeaderboardEntry {
    pub rank: u32,
    pub user_id: u64,
    pub display_name: String,
    pub avatar_url: Option<String>,
    pub duration_seconds: i64,
}

/// Result of generating a leaderboard page.
pub struct PageGenerationResult {
    pub entries_with_names: Vec<(VoiceLeaderboardEntry, String)>,
    pub image_bytes: Vec<u8>,
}

/// Cog for voice tracking commands.
pub struct VoiceCog;

impl VoiceCog {
    /// Voice channel tracking and leaderboard commands
    ///
    /// Track voice channel activity and view leaderboards.
    /// Use subcommands to configure settings or view the leaderboard.
    #[poise::command(slash_command, subcommands("Self::settings", "Self::leaderboard"))]
    pub async fn vc(_ctx: Context<'_>) -> Result<(), Error> {
        Ok(())
    }

    /// Configure voice tracking settings for this server
    ///
    /// Enable or disable voice channel activity tracking.
    /// Only server administrators can use this command.
    #[poise::command(
        slash_command,
        default_member_permissions = "ADMINISTRATOR | MANAGE_GUILD"
    )]
    pub async fn settings(ctx: Context<'_>) -> Result<(), Error> {
        controllers::settings(ctx).await
    }

    /// Display the voice activity leaderboard
    ///
    /// Shows a ranked list of users by total time spent in voice channels.
    /// Includes your current rank position.
    #[poise::command(slash_command)]
    pub async fn leaderboard(ctx: Context<'_>) -> Result<(), Error> {
        controllers::leaderboard(ctx).await
    }
}

impl Cog for VoiceCog {
    fn commands(&self) -> Vec<poise::Command<crate::bot::Data, crate::bot::commands::Error>> {
        vec![Self::vc()]
    }
}
