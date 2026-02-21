//! Navigation system for the MVC-C pattern.
//!
//! Provides unified navigation enum for cross-domain controller navigation.

use crate::bot::commands::feed::SendInto;

/// Result type for controller navigation.
///
/// Controllers return this enum to indicate where the coordinator should
/// navigate next. Each domain (Settings, Feed, Voice) has its own section.
#[derive(Debug, Clone)]
pub enum NavigationResult {
    // Settings section
    /// Navigate to main settings page
    SettingsMain,
    /// Navigate to feed settings page
    SettingsFeeds,
    /// Navigate to voice settings page
    SettingsVoice,
    /// Navigate to welcome settings page
    SettingsWelcome,
    /// Navigate to about page (within settings context)
    SettingsAbout,

    // Feed commands section
    /// Show subscriptions list
    FeedSubscriptions { send_into: Option<SendInto> },
    /// Start subscribe flow
    FeedSubscribe {
        links: String,
        send_into: Option<SendInto>,
    },
    /// Start unsubscribe flow
    FeedUnsubscribe {
        links: String,
        send_into: Option<SendInto>,
    },
    /// Start subscription list flow
    FeedList(Option<SendInto>),

    // Voice commands section
    /// Show voice leaderboard
    VoiceLeaderboard,

    // Universal navigation
    /// Go back to previous controller (uses coordinator's stack)
    Back,
    /// Exit current coordinator session
    Exit,
}
