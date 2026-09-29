//! The voice plugin's domain, storage, commands, and view cores.

use chrono::DateTime;
use chrono::Datelike;
use chrono::Duration as ChronoDuration;
use chrono::Utc;
use pwr_plugin_protocol::API_VERSION;
use pwr_plugin_protocol::CommandDef;
use pwr_plugin_protocol::Manifest;
use pwr_plugin_protocol::SettingsSection;
use serde::Deserialize;
use serde::Serialize;
use serde_json::json;

pub mod command;
pub mod entity;
pub mod heartbeat;
pub mod host_client;
pub mod leaderboard;
pub mod repo;
pub mod service;
pub mod stats;
pub mod storage;
pub mod subscriber;
pub mod update;
pub mod utils;
pub mod view;

pub use entity::GuildDailyStats;
pub use entity::VoiceDailyActivity;
pub use entity::VoiceLeaderboardEntry;
pub use entity::VoiceLeaderboardOpt;
pub use entity::VoiceLeaderboardOptBuilder;
pub use entity::VoiceSessionsEntity;
pub use entity::VoiceSettings;
pub use entity::VoiceSettingsEntity;
pub use service::VoiceTrackingService;

/// The plugin's name and hello identity.
pub const PLUGIN_NAME: &str = "voice";
/// The direct command used by the host Settings section.
pub const COMMAND_NAME: &str = "voice-settings";
/// The qualified public command paths.
pub const VOICE_COMMAND_NAME: &str = "vc";
pub const VOICE_SETTINGS_COMMAND_NAME: &str = "vc settings";
pub const VOICE_LEADERBOARD_COMMAND_NAME: &str = "vc leaderboard";
pub const VOICE_STATS_COMMAND_NAME: &str = "vc stats";

/// Server statistic shown by the stats view.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum GuildStatType {
    /// Average voice time per active user.
    #[default]
    AverageTime,
    /// Number of unique active users.
    ActiveUserCount,
    /// Total voice time.
    TotalTime,
}

impl GuildStatType {
    /// Returns the label shown in a Discord choice.
    pub fn display_name(self) -> &'static str {
        match self {
            Self::AverageTime => "Average Time",
            Self::ActiveUserCount => "Active Users",
            Self::TotalTime => "Total Time",
        }
    }
}

/// Common time-range conversion used by both stats and leaderboard cores.
pub trait TimeRange: Copy {
    /// Returns the inclusive start and end timestamps for this range.
    fn to_range(self, now: DateTime<Utc>) -> (DateTime<Utc>, DateTime<Utc>);
    /// Returns the user-facing label for this range.
    fn display_name(&self) -> &'static str;
    /// Parses a range from its user-facing label.
    fn from_display_name(name: &str) -> Option<Self>;
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum VoiceLeaderboardTimeRange {
    /// From the first day of the current month.
    #[default]
    ThisMonth,
    /// From midnight UTC today.
    Today,
    /// The trailing 24 hours.
    Past24Hours,
    /// The trailing 72 hours.
    Past72Hours,
    /// The trailing seven days.
    Past7Days,
    /// The trailing 14 days.
    Past14Days,
    /// From the first day of the current year.
    ThisYear,
    /// All recorded history.
    AllTime,
}

impl TimeRange for VoiceLeaderboardTimeRange {
    fn to_range(self, now: DateTime<Utc>) -> (DateTime<Utc>, DateTime<Utc>) {
        let since = match self {
            Self::Today => now.date_naive().and_hms_opt(0, 0, 0).unwrap().and_utc(),
            Self::Past24Hours => now - ChronoDuration::hours(24),
            Self::Past72Hours => now - ChronoDuration::hours(72),
            Self::Past7Days => now - ChronoDuration::days(7),
            Self::Past14Days => now - ChronoDuration::days(14),
            Self::ThisMonth => now
                .date_naive()
                .with_day(1)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap()
                .and_utc(),
            Self::ThisYear => now
                .date_naive()
                .with_month(1)
                .unwrap()
                .with_day(1)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap()
                .and_utc(),
            Self::AllTime => DateTime::UNIX_EPOCH,
        };
        (since, now)
    }

    fn display_name(&self) -> &'static str {
        match self {
            Self::ThisMonth => "This month",
            Self::Today => "Today",
            Self::Past24Hours => "Past 24 hours",
            Self::Past72Hours => "Past 72 hours",
            Self::Past7Days => "Past 7 days",
            Self::Past14Days => "Past 14 days",
            Self::ThisYear => "This year",
            Self::AllTime => "All time",
        }
    }

    fn from_display_name(name: &str) -> Option<Self> {
        Self::all()
            .into_iter()
            .find(|range| range.display_name() == name)
    }
}

impl VoiceLeaderboardTimeRange {
    /// Returns every selectable leaderboard range.
    pub const fn all() -> [Self; 8] {
        [
            Self::Past24Hours,
            Self::Past72Hours,
            Self::Past7Days,
            Self::Past14Days,
            Self::ThisMonth,
            Self::ThisYear,
            Self::AllTime,
            Self::Today,
        ]
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum VoiceStatsTimeRange {
    /// The trailing 365 days.
    Yearly,
    /// The trailing 120 days.
    #[default]
    Monthly,
    /// The trailing 28 days.
    Weekly,
    /// The trailing four days.
    Hourly,
}

impl TimeRange for VoiceStatsTimeRange {
    fn to_range(self, now: DateTime<Utc>) -> (DateTime<Utc>, DateTime<Utc>) {
        let since = match self {
            Self::Yearly => now - ChronoDuration::days(365),
            Self::Monthly => now - ChronoDuration::days(120),
            Self::Weekly => now - ChronoDuration::days(28),
            Self::Hourly => now - ChronoDuration::days(4),
        };
        (since, now)
    }

    fn display_name(&self) -> &'static str {
        match self {
            Self::Yearly => "Yearly",
            Self::Monthly => "Monthly",
            Self::Weekly => "Weekly",
            Self::Hourly => "Hourly",
        }
    }

    fn from_display_name(name: &str) -> Option<Self> {
        match name {
            "Yearly" => Some(Self::Yearly),
            "Monthly" => Some(Self::Monthly),
            "Weekly" => Some(Self::Weekly),
            "Hourly" => Some(Self::Hourly),
            _ => None,
        }
    }
}

fn choice(name: &str, value: i64) -> serde_json::Value {
    json!({ "name": name, "value": value })
}

/// The voice plugin's manifest.
pub fn manifest() -> Manifest {
    let leaderboard_range = json!({
        "name": "time_range",
        "description": "Time period to filter voice activity. Defaults to \"This month\"",
        "type": 4,
        "required": false,
        "choices": [
            choice("This month", 0),
            choice("Today", 1),
            choice("Past 24 hours", 2),
            choice("Past 72 hours", 3),
            choice("Past 7 days", 4),
            choice("Past 14 days", 5),
            choice("This year", 6),
            choice("All time", 7),
        ],
    });
    let stats_range = json!({
        "name": "time_range",
        "description": "Time period to display. Defaults to \"This month\"",
        "type": 4,
        "required": false,
        "choices": [
            choice("Yearly", 0),
            choice("Monthly", 1),
            choice("Weekly", 2),
            choice("Hourly", 3),
        ],
    });
    Manifest {
        name: PLUGIN_NAME.into(),
        description: "Manage voice tracking settings".into(),
        version: "0.1.0".into(),
        commands: vec![
            CommandDef {
                create_command: json!({
                    "name": VOICE_COMMAND_NAME,
                    "description": "Voice channel tracking and leaderboard commands",
                    "options": [
                        {
                            "name": "settings",
                            "description": "Configure voice tracking settings for this server",
                            "type": 1,
                            "default_member_permissions": "40",
                        },
                        {
                            "name": "leaderboard",
                            "description": "Display the voice activity leaderboard",
                            "type": 1,
                            "options": [leaderboard_range],
                        },
                        {
                            "name": "stats",
                            "description": "Show voice activity statistics",
                            "type": 1,
                            "options": [
                                stats_range,
                                {
                                    "name": "user",
                                    "description": concat!(
                                        "User to show stats for (defaults to server stats in ",
                                        "server, yourself in DM)"
                                    ),
                                    "type": 6,
                                    "required": false,
                                },
                                {
                                    "name": "statistic",
                                    "description": "Statistic to display for server view",
                                    "type": 4,
                                    "required": false,
                                    "choices": [
                                        choice("Average Time", 0),
                                        choice("Active Users", 1),
                                        choice("Total Time", 2),
                                    ],
                                },
                            ],
                        },
                    ],
                }),
            },
            CommandDef {
                create_command: json!({
                    "name": COMMAND_NAME,
                    "description": "Manage voice tracking settings",
                    "dm_permission": false,
                    "guild_only": true,
                    "default_member_permissions": "40",
                }),
            },
        ],
        event_handlers: vec![
            "voice_state".into(),
            "guild_create".into(),
            "view.timeout".into(),
        ],
        tasks: vec![],
        settings: vec![SettingsSection {
            name: "Voice".into(),
            description: "Manage voice tracking settings".into(),
            command: COMMAND_NAME.into(),
        }],
        requires: vec![],
        api_version: API_VERSION,
    }
}
