//! The `host.stats` payload: the live bot statistics a plugin receives from
//! the [`crate::HostCap::Stats`] op. Mirrors what the host's `/about` command
//! shows, minus render-only fields (the About view's `current_year`): the
//! plugin formats the values itself.

use serde::Deserialize;
use serde::Serialize;

/// The live bot statistics served by the `host.stats` op.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HostStats {
    /// Bot version string, e.g. `0.4.2`.
    pub version: String,
    /// Seconds since the host process started.
    pub uptime_secs: u64,
    /// Number of guilds the bot is in.
    pub guild_count: u64,
    /// Sum of the cached per-guild member counts.
    pub user_count: u64,
    /// Live REST round-trip latency in milliseconds, sampled when the host
    /// gathers the snapshot (the same probe `/about` makes).
    pub latency_ms: u64,
    /// Number of registered commands, subcommands included.
    pub command_count: u64,
    /// Resident process memory in megabytes.
    pub memory_mb: f64,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn host_stats_serializes_to_declared_shape() {
        let stats = HostStats {
            version: "0.4.2".into(),
            uptime_secs: 90_000,
            guild_count: 2,
            user_count: 1_500,
            latency_ms: 42,
            command_count: 12,
            memory_mb: 320.0,
        };
        assert_eq!(
            serde_json::to_value(&stats).unwrap(),
            json!({
                "version": "0.4.2",
                "uptime_secs": 90_000,
                "guild_count": 2,
                "user_count": 1_500,
                "latency_ms": 42,
                "command_count": 12,
                "memory_mb": 320.0,
            })
        );
    }

    #[test]
    fn host_stats_round_trips_losslessly() {
        let stats = HostStats {
            version: "1.0.0".into(),
            uptime_secs: 0,
            guild_count: 0,
            user_count: 0,
            latency_ms: 0,
            command_count: 0,
            memory_mb: 0.0,
        };
        let json = serde_json::to_string(&stats).unwrap();
        assert_eq!(serde_json::from_str::<HostStats>(&json).unwrap(), stats);
    }
}
