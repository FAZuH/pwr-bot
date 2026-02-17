//! Voice channel tracking and leaderboard commands.

use chrono::DateTime;
use chrono::Datelike;
use chrono::Duration;
use chrono::Utc;
use poise::ChoiceParameter;

use crate::bot::commands::Cog;
use crate::bot::commands::Context;
use crate::bot::commands::Error;
use crate::bot::views::Action;

pub mod controllers;
pub mod image_builder;
pub mod image_generator;
pub mod views;

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
    pub async fn leaderboard(
        ctx: Context<'_>,
        #[description = "Time period to filter voice activity. Defaults to \"This month\""]
        time_range: Option<VoiceLeaderboardTimeRange>,
    ) -> Result<(), Error> {
        controllers::leaderboard(
            ctx,
            time_range.unwrap_or(VoiceLeaderboardTimeRange::ThisMonth),
        )
        .await
    }
}

impl Cog for VoiceCog {
    fn commands(&self) -> Vec<poise::Command<crate::bot::Data, crate::bot::commands::Error>> {
        vec![Self::vc()]
    }
}

/// Time range filter for voice activity leaderboard.
#[derive(ChoiceParameter, Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoiceLeaderboardTimeRange {
    /// From 00:00 today until now
    Today,
    /// From 00:00 3 days ago until now
    #[name = "Past 3 days"]
    Past3Days,
    /// From Sunday 00:00 until now
    #[name = "This week"]
    ThisWeek,
    /// From Sunday 00:00 two weeks ago until now
    #[name = "Past 2 weeks"]
    Past2Weeks,
    /// From 1st of this month 00:00 until now
    #[name = "This month"]
    ThisMonth,
    /// From January 1st 00:00 until now
    #[name = "This year"]
    ThisYear,
    /// All recorded history
    #[name = "All time"]
    AllTime,
}

impl Action for VoiceLeaderboardTimeRange {
    fn label(&self) -> &'static str {
        self.name()
    }
}

impl From<VoiceLeaderboardTimeRange> for DateTime<Utc> {
    fn from(range: VoiceLeaderboardTimeRange) -> Self {
        let now = Utc::now();

        match range {
            VoiceLeaderboardTimeRange::Today => {
                // From 00:00 today
                now.date_naive().and_hms_opt(0, 0, 0).unwrap().and_utc()
            }

            VoiceLeaderboardTimeRange::Past3Days => {
                // From 00:00 3 days ago (today + 2 days ago = 3 days total)
                (now - Duration::days(2))
                    .date_naive()
                    .and_hms_opt(0, 0, 0)
                    .unwrap()
                    .and_utc()
            }

            VoiceLeaderboardTimeRange::ThisWeek => {
                // From Sunday 00:00 of this week
                let days_since_sunday = now.weekday().num_days_from_sunday();
                (now - Duration::days(days_since_sunday as i64))
                    .date_naive()
                    .and_hms_opt(0, 0, 0)
                    .unwrap()
                    .and_utc()
            }

            VoiceLeaderboardTimeRange::Past2Weeks => {
                // From Sunday 00:00 of previous week
                let days_since_sunday = now.weekday().num_days_from_sunday();
                let days_to_subtract = days_since_sunday as i64 + 7; // Go back to previous Sunday
                (now - Duration::days(days_to_subtract))
                    .date_naive()
                    .and_hms_opt(0, 0, 0)
                    .unwrap()
                    .and_utc()
            }

            VoiceLeaderboardTimeRange::ThisMonth => {
                // From 00:00 on 1st of this month
                now.date_naive()
                    .with_day(1)
                    .unwrap()
                    .and_hms_opt(0, 0, 0)
                    .unwrap()
                    .and_utc()
            }

            VoiceLeaderboardTimeRange::ThisYear => {
                // From 00:00 on January 1st this year
                now.date_naive()
                    .with_month(1)
                    .unwrap()
                    .with_day(1)
                    .unwrap()
                    .and_hms_opt(0, 0, 0)
                    .unwrap()
                    .and_utc()
            }

            VoiceLeaderboardTimeRange::AllTime => DateTime::UNIX_EPOCH,
        }
    }
}

// Helper to get both since and until
impl VoiceLeaderboardTimeRange {
    /// Returns (since, until) where until is always now
    pub fn to_range(self) -> (DateTime<Utc>, DateTime<Utc>) {
        (self.into(), Utc::now())
    }

    /// Returns the user-facing name for this time range.
    pub fn name(&self) -> &'static str {
        match self {
            VoiceLeaderboardTimeRange::Today => "Today",
            VoiceLeaderboardTimeRange::Past3Days => "Past 3 days",
            VoiceLeaderboardTimeRange::ThisWeek => "This week",
            VoiceLeaderboardTimeRange::Past2Weeks => "Past 2 weeks",
            VoiceLeaderboardTimeRange::ThisMonth => "This month",
            VoiceLeaderboardTimeRange::ThisYear => "This year",
            VoiceLeaderboardTimeRange::AllTime => "All time",
        }
    }

    /// Returns the user-facing name for this time range.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "Today" => Some(VoiceLeaderboardTimeRange::Today),
            "Past 3 days" => Some(VoiceLeaderboardTimeRange::Past3Days),
            "This week" => Some(VoiceLeaderboardTimeRange::ThisWeek),
            "Past 2 weeks" => Some(VoiceLeaderboardTimeRange::Past2Weeks),
            "This month" => Some(VoiceLeaderboardTimeRange::ThisMonth),
            "This year" => Some(VoiceLeaderboardTimeRange::ThisYear),
            "All time" => Some(VoiceLeaderboardTimeRange::AllTime),
            _ => None,
        }
    }
}
