//! Voice channel tracking and leaderboard commands.

use chrono::DateTime;
use chrono::Datelike;
use chrono::Duration;
use chrono::Utc;
use poise::ChoiceParameter;
use serenity::all::CreateSelectMenuOption;

use crate::bot::commands::Context;
use crate::bot::commands::Error;
use crate::bot::views::Action;

pub mod leaderboard;
pub mod settings;
pub mod stats;
pub mod stats_chart;

/// Voice channel tracking and leaderboard commands
///
/// Track voice channel activity and view leaderboards.
/// Use subcommands to configure settings or view the leaderboard.
#[poise::command(
    slash_command,
    rename = "vc",
    subcommands("settings::settings", "leaderboard::leaderboard", "stats::stats")
)]
pub async fn voice(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// Type of guild statistic to display.
#[derive(ChoiceParameter, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum GuildStatType {
    /// Average voice time per active user
    #[default]
    #[name = "Average Time"]
    AverageTime,
    /// Number of unique active users
    #[name = "Active Users"]
    ActiveUserCount,
    /// Total voice time
    #[name = "Total Time"]
    TotalTime,
}

/// Common trait for time range enums.
pub trait TimeRange: Copy {
    /// Returns (since, until) where until is always now.
    fn to_range(self) -> (DateTime<Utc>, DateTime<Utc>);
    /// Returns the user-facing display name for this time range.
    fn display_name(&self) -> &'static str;
    /// Returns the enum variant from its display name.
    fn from_display_name(name: &str) -> Option<Self>;
}

/// Time range filter for voice activity leaderboard.
#[derive(ChoiceParameter, Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoiceLeaderboardTimeRange {
    /// From 24 hours ago until now
    #[name = "Past 24 hours"]
    Past24Hours,
    /// From 72 hours ago until now
    #[name = "Past 72 hours"]
    Past72Hours,
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
            VoiceLeaderboardTimeRange::Past24Hours => now - Duration::hours(24),

            VoiceLeaderboardTimeRange::Past72Hours => now - Duration::hours(72),

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
    fn to_date_time(self) -> DateTime<Utc> {
        self.into()
    }
}

impl TimeRange for VoiceLeaderboardTimeRange {
    fn to_range(self) -> (DateTime<Utc>, DateTime<Utc>) {
        (self.to_date_time(), Utc::now())
    }

    fn display_name(&self) -> &'static str {
        match self {
            VoiceLeaderboardTimeRange::Past24Hours => "Past 24 hours",
            VoiceLeaderboardTimeRange::Past72Hours => "Past 72 hours",
            VoiceLeaderboardTimeRange::ThisWeek => "This week",
            VoiceLeaderboardTimeRange::Past2Weeks => "Past 2 weeks",
            VoiceLeaderboardTimeRange::ThisMonth => "This month",
            VoiceLeaderboardTimeRange::ThisYear => "This year",
            VoiceLeaderboardTimeRange::AllTime => "All time",
        }
    }

    fn from_display_name(name: &str) -> Option<Self> {
        match name {
            "Past 24 hours" => Some(VoiceLeaderboardTimeRange::Past24Hours),
            "Past 72 hours" => Some(VoiceLeaderboardTimeRange::Past72Hours),
            "This week" => Some(VoiceLeaderboardTimeRange::ThisWeek),
            "Past 2 weeks" => Some(VoiceLeaderboardTimeRange::Past2Weeks),
            "This month" => Some(VoiceLeaderboardTimeRange::ThisMonth),
            "This year" => Some(VoiceLeaderboardTimeRange::ThisYear),
            "All time" => Some(VoiceLeaderboardTimeRange::AllTime),
            _ => None,
        }
    }
}

/// Time range filter for voice stats.
#[derive(ChoiceParameter, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum VoiceStatsTimeRange {
    /// From 1 year ago until now
    #[default]
    #[name = "Yearly"]
    Yearly,
    /// From 4 months ago until now
    #[name = "Monthly"]
    Monthly,
    /// From 4 weeks ago until now
    #[name = "Weekly"]
    Weekly,
    /// From 4 days ago until now
    #[name = "Hourly"]
    Hourly,
}

impl VoiceStatsTimeRange {
    fn to_date_time(self) -> DateTime<Utc> {
        let now = Utc::now();

        match self {
            VoiceStatsTimeRange::Yearly => now - Duration::days(365),
            VoiceStatsTimeRange::Monthly => {
                now - Duration::days(30 * 4) // approx 4 months
            }
            VoiceStatsTimeRange::Weekly => {
                now - Duration::days(7 * 4) // 4 weeks
            }
            VoiceStatsTimeRange::Hourly => {
                now - Duration::days(4) // 4 days
            }
        }
    }
}

impl TimeRange for VoiceStatsTimeRange {
    fn to_range(self) -> (DateTime<Utc>, DateTime<Utc>) {
        (self.to_date_time(), Utc::now())
    }

    fn display_name(&self) -> &'static str {
        match self {
            VoiceStatsTimeRange::Yearly => "Yearly",
            VoiceStatsTimeRange::Monthly => "Monthly",
            VoiceStatsTimeRange::Weekly => "Weekly",
            VoiceStatsTimeRange::Hourly => "Hourly",
        }
    }

    fn from_display_name(name: &str) -> Option<Self> {
        match name {
            "Yearly" => Some(VoiceStatsTimeRange::Yearly),
            "Monthly" => Some(VoiceStatsTimeRange::Monthly),
            "Weekly" => Some(VoiceStatsTimeRange::Weekly),
            "Hourly" => Some(VoiceStatsTimeRange::Hourly),
            _ => None,
        }
    }
}

impl From<VoiceStatsTimeRange> for CreateSelectMenuOption<'static> {
    fn from(range: VoiceStatsTimeRange) -> Self {
        let name = range.display_name();
        CreateSelectMenuOption::new(name, name)
    }
}
