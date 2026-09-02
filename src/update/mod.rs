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

pub mod about;
pub mod feed_batch;
pub mod feed_list;
pub mod feed_settings;
pub mod plugins;
pub mod register;
pub mod unregister;
pub mod voice_leaderboard;
pub mod voice_settings;
pub mod voice_stats;
pub mod welcome_settings;

pub use about::AboutEffect;
pub use about::AboutModel;
pub use about::AboutMsg;
pub use about::AboutStats;
pub use about::update as about_update;
pub use feed_batch::FeedBatchEffect;
pub use feed_batch::FeedBatchModel;
pub use feed_batch::FeedBatchMsg;
pub use feed_batch::FeedBatchPhase;
pub use feed_batch::update as feed_batch_update;
pub use feed_list::FeedListCmd;
pub use feed_list::FeedListModel;
pub use feed_list::FeedListMsg;
pub use feed_list::FeedListUpdate;
pub use feed_list::FeedListViewState;
pub use feed_settings::FeedSettingsCmd;
pub use feed_settings::FeedSettingsModel;
pub use feed_settings::FeedSettingsMsg;
pub use feed_settings::FeedSettingsUpdate;
pub use plugins::PluginsCmd;
pub use plugins::PluginsModel;
pub use plugins::PluginsMsg;
pub use plugins::PluginsUpdate;
pub use register::RegisterEffect;
pub use register::RegisterModel;
pub use register::RegisterMsg;
pub use register::update as register_update;
pub use unregister::UnregisterEffect;
pub use unregister::UnregisterModel;
pub use unregister::UnregisterMsg;
pub use unregister::update as unregister_update;
pub use voice_settings::VoiceSettingsEffect;
pub use voice_settings::VoiceSettingsModel;
pub use voice_settings::VoiceSettingsMsg;
pub use voice_settings::update as voice_settings_update;
pub use voice_stats::VoiceStatsCmd;
pub use voice_stats::VoiceStatsModel;
pub use voice_stats::VoiceStatsMsg;
pub use voice_stats::VoiceStatsUpdate;
pub use welcome_settings::WelcomeSettingsCmd;
pub use welcome_settings::WelcomeSettingsModel;
pub use welcome_settings::WelcomeSettingsMsg;
pub use welcome_settings::WelcomeSettingsUpdate;
