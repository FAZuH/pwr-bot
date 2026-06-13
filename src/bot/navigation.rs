//! Navigation system for bot command router.
//!
//! Provides unified navigation enum for cross-domain handler navigation.

use crate::bot::command::feed::SendInto;

/// Result type for handler navigation.
///
/// Handlers return this enum to indicate where the coordinator should
/// navigate next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Navigation {
    // -- Settings section --
    /// Navigate to main settings page
    SettingsMain,
    /// Navigate to feed settings page
    SettingsFeeds,
    /// Navigate to voice settings page
    SettingsVoice,
    /// Navigate to welcome settings page
    SettingsWelcome,
    /// Navigate to a plugin-contributed settings page
    SettingsPlugin {
        /// ID of the plugin settings panel.
        plugin_id: String,
    },
    /// Navigate to about page (within settings context)
    SettingsAbout,

    // -- Feed commands section --
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
}
