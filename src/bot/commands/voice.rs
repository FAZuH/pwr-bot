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

/// Type of server statistic to display.
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

impl<T: poise::ChoiceParameter + Copy + Into<DateTime<Utc>>> TimeRange for T {
    fn to_range(self) -> (DateTime<Utc>, DateTime<Utc>) {
        (Into::<DateTime<Utc>>::into(self), Utc::now())
    }

    fn display_name(&self) -> &'static str {
        self.name()
    }

    fn from_display_name(name: &str) -> Option<Self> {
        Self::from_name(name)
    }
}

/// Time range filter for voice activity leaderboard.
#[derive(ChoiceParameter, Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoiceLeaderboardTimeRange {
    /// From start of today (UTC 00:00) until now
    #[name = "Today"]
    Today,
    /// From 24 hours ago until now
    #[name = "Past 24 hours"]
    Past24Hours,
    /// From 72 hours ago until now
    #[name = "Past 72 hours"]
    Past72Hours,
    /// From 7 days ago until now
    #[name = "Past 7 days"]
    Past7Days,
    /// From 14 days ago until now
    #[name = "Past 14 days"]
    Past14Days,
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
                now.date_naive().and_hms_opt(0, 0, 0).unwrap().and_utc()
            }
            VoiceLeaderboardTimeRange::Past24Hours => now - Duration::hours(24),
            VoiceLeaderboardTimeRange::Past72Hours => now - Duration::hours(72),
            VoiceLeaderboardTimeRange::Past7Days => now - Duration::days(7),
            VoiceLeaderboardTimeRange::Past14Days => now - Duration::days(14),
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

impl From<VoiceStatsTimeRange> for DateTime<Utc> {
    fn from(value: VoiceStatsTimeRange) -> Self {
        let now = Utc::now();

        match value {
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

impl From<VoiceStatsTimeRange> for CreateSelectMenuOption<'static> {
    fn from(range: VoiceStatsTimeRange) -> Self {
        let name = range.display_name();
        CreateSelectMenuOption::new(name, name)
    }
}
