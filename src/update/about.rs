//! Pure update logic for the `/about` command.
//!
//! Holds the single source of truth for the about view (`AboutModel`), the
//! exhaustive message vocabulary (`AboutMsg`), and an empty effect vocabulary
//! (`AboutEffect`). The view is static — it shows bot statistics and offers a
//! back button — so `update` never mutates the model and never performs IO.
//!
//! IO (gathering stats, current avatar) lives in the shell layer
//! ([`crate::bot::command::about`] and [`crate::bot::gui::about`]).

use std::time::Duration;

use crate::update::lifecycle::Lifecycle;

/// Statistics displayed on the about view.
#[derive(Debug, Clone)]
pub struct AboutStats {
    pub(crate) version: String,
    pub(crate) uptime: Duration,
    pub(crate) guild_count: usize,
    pub(crate) user_count: usize,
    pub(crate) latency_ms: u64,
    pub(crate) command_count: usize,
    pub(crate) memory_mb: f64,
    pub(crate) current_year: i32,
}

impl AboutStats {
    /// Builds the stats. Public so the shell layer can construct the model
    /// from data gathered outside the pure core.
    #[allow(clippy::too_many_arguments)] // pure data-in constructor mirroring the gathered stats
    pub fn new(
        version: String,
        uptime: Duration,
        guild_count: usize,
        user_count: usize,
        latency_ms: u64,
        command_count: usize,
        memory_mb: f64,
        current_year: i32,
    ) -> Self {
        Self {
            version,
            uptime,
            guild_count,
            user_count,
            latency_ms,
            command_count,
            memory_mb,
            current_year,
        }
    }
}

/// The about view model — the single source of truth for the view state.
#[derive(Debug, Clone)]
pub struct AboutModel {
    pub(crate) stats: AboutStats,
    pub(crate) avatar_url: String,
}

impl AboutModel {
    /// Constructs the about model from the stats and avatar URL.
    pub fn new(stats: AboutStats, avatar_url: String) -> Self {
        Self { stats, avatar_url }
    }

    /// Formats a duration into a human-readable uptime string.
    pub fn format_uptime(duration: Duration) -> String {
        let days = duration.as_secs() / 86400;
        let hours = (duration.as_secs() % 86400) / 3600;
        let minutes = (duration.as_secs() % 3600) / 60;

        if days > 0 {
            format!("{days} days, {hours} hours, {minutes} minutes")
        } else if hours > 0 {
            format!("{hours} hours, {minutes} minutes")
        } else {
            format!("{minutes} minutes")
        }
    }

    /// Formats a number with k/M suffixes for readability.
    pub fn format_number(num: usize) -> String {
        if num >= 1_000_000 {
            format!("{:.1}M", num as f64 / 1_000_000.0)
        } else if num >= 1_000 {
            format!("{:.1}k", num as f64 / 1_000.0)
        } else {
            num.to_string()
        }
    }
}

/// Messages that drive the about view.
///
/// Exhaustive: every way the world can change the about model is one variant.
/// The lifecycle moments share the wrapped [`Lifecycle`] form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AboutMsg {
    /// The shared boot/timeout lifecycle moments.
    Lifecycle(Lifecycle),
    /// The back button was pressed.
    Back,
}

impl From<Lifecycle> for AboutMsg {
    fn from(lifecycle: Lifecycle) -> Self {
        Self::Lifecycle(lifecycle)
    }
}

/// Effects the about view can request.
///
/// Empty: the about view performs no side effects (data is supplied at model
/// construction; navigation is a host concern handled via [`AboutMsg`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AboutEffect {}

/// The pure update function. The about view never mutates its model, so every
/// message is a no-op that returns no effects (expiry included — the about
/// view persists nothing).
pub fn update(msg: AboutMsg, _model: &mut AboutModel) -> Vec<AboutEffect> {
    match msg {
        AboutMsg::Lifecycle(lifecycle) => lifecycle.handle(Vec::new),
        AboutMsg::Back => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> AboutModel {
        AboutModel::new(
            AboutStats::new(
                "0.1.0".to_string(),
                Duration::from_secs(90_000),
                2,
                150,
                42,
                12,
                320.0,
                2026,
            ),
            "https://example.com/avatar.png".to_string(),
        )
    }

    #[test]
    fn start_is_a_noop() {
        let mut m = model();
        let effects = update(AboutMsg::Lifecycle(Lifecycle::Start), &mut m);
        assert!(effects.is_empty());
    }

    #[test]
    fn expired_is_a_noop() {
        let mut m = model();
        let effects = update(AboutMsg::Lifecycle(Lifecycle::Expired), &mut m);
        assert!(effects.is_empty());
    }

    #[test]
    fn back_is_a_noop() {
        let mut m = model();
        let effects = update(AboutMsg::Back, &mut m);
        assert!(effects.is_empty());
    }

    #[test]
    fn format_uptime_days() {
        assert_eq!(
            AboutModel::format_uptime(Duration::from_secs(90_000)),
            "1 days, 1 hours, 0 minutes"
        );
    }

    #[test]
    fn format_uptime_hours() {
        assert_eq!(
            AboutModel::format_uptime(Duration::from_secs(3600)),
            "1 hours, 0 minutes"
        );
    }

    #[test]
    fn format_uptime_minutes() {
        assert_eq!(
            AboutModel::format_uptime(Duration::from_secs(300)),
            "5 minutes"
        );
    }

    #[test]
    fn format_number_suffixes() {
        assert_eq!(AboutModel::format_number(999), "999");
        assert_eq!(AboutModel::format_number(1_500), "1.5k");
        assert_eq!(AboutModel::format_number(2_000_000), "2.0M");
    }
}
