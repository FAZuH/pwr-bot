//! Navigation system for bot command router.
//!
//! Provides unified navigation enum for cross-domain handler navigation.

/// Result type for handler navigation.
///
/// Handlers return this enum to indicate where the coordinator should
/// navigate next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Navigation {
    // -- Settings section --
    /// Navigate to main settings page
    SettingsMain,
    /// Navigate to a plugin-contributed settings page
    SettingsPlugin {
        /// ID of the plugin settings panel.
        plugin_id: String,
    },
    /// Navigate to about page (within settings context)
    SettingsAbout,
}
