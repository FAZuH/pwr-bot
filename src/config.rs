//! Configuration management for the bot.
//!
//! Handles loading configuration from environment variables.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use log::info;

use crate::error::AppError;

/// Bot configuration loaded from environment variables.
#[derive(Clone, Default, Debug)]
pub struct Config {
    pub poll_interval: Duration,
    pub db_url: String,
    pub discord_token: String,
    pub discord_application_id: Option<u64>,
    pub admin_id: String,
    pub data_path: PathBuf,
    pub logs_path: PathBuf,
    pub plugin_dir: PathBuf,
    pub features: Features,
    pub version: String,
}

/// Dynamic feature flags loaded from `ENABLE_*` environment variables.
///
/// Scans all environment variables at startup, matches those starting with
/// `ENABLE_`, strips the prefix, lowercases the remainder, and parses the
/// value as a boolean. Plugins can query arbitrary feature names via
/// [`is_enabled`](Features::is_enabled).
#[derive(Clone, Default, Debug)]
pub struct Features {
    flags: HashMap<String, bool>,
}

impl Features {
    /// Creates features from a map of flag name → enabled status.
    pub fn new(flags: HashMap<String, bool>) -> Self {
        Self { flags }
    }

    /// Returns `true` if the named feature is enabled.
    pub fn is_enabled(&self, name: &str) -> bool {
        self.flags.get(name).copied().unwrap_or(false)
    }

    /// Loads all `ENABLE_*` environment variables.
    ///
    /// Accepts: `"true"`, `"1"`, `"yes"`, `"on"` (case-insensitive) as truthy.
    /// Unset variables default to `false` (the map lookup returns `None` →
    /// [`is_enabled`] returns `false`).
    ///
    /// Backward-compatible defaults for the original three feature flags are
    /// applied only when the corresponding env var is not set:
    ///
    /// | Env var | Feature key | Default |
    /// |---|---|---|
    /// | `ENABLE_VOICE_TRACKING` | `voice_tracking` | `true` |
    /// | `ENABLE_FEED_PUBLISHER` | `feed_publisher` | `true` |
    /// | `ENABLE_AUTOREGISTER_CMD` | `autoregister_cmds` | `true` |
    fn from_env() -> Self {
        let mut flags = HashMap::new();

        // Backward-compatible defaults for the original three feature flags.
        // These are only used if the env var is not set at all.
        let legacy_defaults: &[(&str, &str, bool)] = &[
            ("ENABLE_VOICE_TRACKING", "voice_tracking", true),
            ("ENABLE_FEED_PUBLISHER", "feed_publisher", true),
            ("ENABLE_AUTOREGISTER_CMD", "autoregister_cmds", true),
        ];

        for (env_var, key, default) in legacy_defaults {
            let val = std::env::var(env_var);
            match val {
                Ok(v) => {
                    flags.insert(key.to_string(), parse_bool(&v));
                }
                Err(_) => {
                    flags.insert(key.to_string(), *default);
                }
            }
        }

        // Scan all env vars for additional ENABLE_* flags
        for (key, val) in std::env::vars() {
            let Some(suffix) = key.strip_prefix("ENABLE_") else {
                continue;
            };
            let name = suffix.to_lowercase();

            // Skip already-processed legacy keys to avoid re-processing
            if flags.contains_key(&name) {
                continue;
            }

            let enabled = parse_bool(val.as_str());
            flags.insert(name, enabled);
        }

        Self { flags }
    }
}

impl Config {
    /// Creates a new empty configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Loads configuration from environment variables.
    pub fn load(&mut self) -> Result<(), AppError> {
        self.poll_interval = std::env::var("POLL_INTERVAL")
            .unwrap_or("60".to_string())
            .parse::<u32>()
            .map_or(Duration::new(60, 0), |v| Duration::new(v.into(), 0));

        self.db_url = std::env::var("DB_URL")
            .unwrap_or("postgres://pwr_bot:pwr_bot@localhost:5432/pwr_bot".to_string());

        self.discord_token =
            std::env::var("DISCORD_TOKEN").map_err(|_| AppError::MissingConfig {
                config: "DISCORD_TOKEN".to_string(),
            })?;
        self.discord_application_id = std::env::var("DISCORD_APPLICATION_ID")
            .ok()
            .map(|v| {
                v.parse::<u64>().map_err(|_| AppError::ConfigurationError {
                    msg: format!("DISCORD_APPLICATION_ID '{v}' is not a valid number"),
                })
            })
            .transpose()?;

        self.admin_id = std::env::var("ADMIN_ID").map_err(|_| AppError::MissingConfig {
            config: "ADMIN_ID".to_string(),
        })?;

        self.data_path = self.get_dirpath_mustexist("DATA_PATH", "./data")?;
        self.logs_path = self.get_dirpath_mustexist("LOGS_PATH", "./logs")?;
        self.plugin_dir = self.get_dirpath_mustexist("PLUGIN_DIR", "./plugins")?;

        self.features = Features::from_env();

        self.version = env!("CARGO_PKG_VERSION").to_string();

        Ok(())
    }

    /// Gets a directory path from environment variable, creating it if needed.
    fn get_dirpath_mustexist(
        &self,
        var: &'static str,
        default: &'static str,
    ) -> Result<PathBuf, AppError> {
        let val = std::env::var(var).unwrap_or(default.to_string());
        let path = PathBuf::from(val);
        let path_str = path.to_string_lossy();

        if !path.exists() {
            info!("Directory {path_str} does not exist. Creating...");
            std::fs::create_dir_all(&path).ok();
        } else if !path.is_dir() {
            return Err(AppError::ConfigurationError {
                msg: format!("Path {path_str} exists but is a file when it must be a directory."),
            });
        }

        Ok(path)
    }
}

/// Parse a boolean string value (not an env var — just the value).
fn parse_bool(val: &str) -> bool {
    matches!(val.to_lowercase().as_str(), "true" | "1" | "yes" | "on")
}
