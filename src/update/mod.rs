//! TEA-style update module for pure business logic.
//!
//! Separates state mutations and side-effect commands from UI rendering,
//! making the core logic fully unit-testable without mocking Discord.

/// The Elm Architecture update trait.
///
/// Receives a message and the current model, mutates the model in-place,
/// and returns a command describing any side effects the caller should perform.
pub trait Update {
    type Model;
    type Msg;
    type Cmd;

    fn update(msg: Self::Msg, model: &mut Self::Model) -> Self::Cmd;
}

pub mod feed_list;
pub mod feed_settings;
pub mod settings_main;
pub mod voice_leaderboard;
pub mod voice_stats;
pub mod welcome_settings;

pub use feed_list::FeedListCmd;
pub use feed_list::FeedListModel;
pub use feed_list::FeedListMsg;
pub use feed_list::FeedListUpdate;
pub use feed_list::FeedListViewState;
pub use feed_settings::FeedSettingsCmd;
pub use feed_settings::FeedSettingsModel;
pub use feed_settings::FeedSettingsMsg;
pub use feed_settings::FeedSettingsUpdate;
pub use settings_main::SettingsMainCmd;
pub use settings_main::SettingsMainModel;
pub use settings_main::SettingsMainMsg;
pub use settings_main::SettingsMainUpdate;
pub use voice_stats::VoiceStatsCmd;
pub use voice_stats::VoiceStatsModel;
pub use voice_stats::VoiceStatsMsg;
pub use voice_stats::VoiceStatsUpdate;
pub use welcome_settings::WelcomeSettingsCmd;
pub use welcome_settings::WelcomeSettingsModel;
pub use welcome_settings::WelcomeSettingsMsg;
pub use welcome_settings::WelcomeSettingsUpdate;
