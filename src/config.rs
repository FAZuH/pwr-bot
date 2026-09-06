//! Configuration management for the bot.
//!
//! Handles loading configuration from environment variables.

use std::path::PathBuf;
use std::time::Duration;

use log::info;
use log::warn;

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
    pub plugins_toml: PathBuf,
    pub plugins_dir: PathBuf,
    pub settings_plugin_path: PathBuf,
    pub feed_settings_plugin_path: PathBuf,
    pub voice_settings_plugin_path: PathBuf,
    /// Core plugins the host spawns at startup, in order.
    pub core_plugins: Vec<CorePluginSpec>,
    pub features: Features,
    pub version: String,
}

/// One core plugin the host spawns at startup: its name and binary path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CorePluginSpec {
    /// Plugin name, e.g. `settings`.
    pub name: String,
    /// Binary path to spawn.
    pub path: PathBuf,
}

/// Feature flags for optional bot components.
#[derive(Clone, Default, Debug)]
pub struct Features {
    pub voice_tracking: bool,
    pub feed_publisher: bool,
    pub autoregister_cmds: bool,
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
        self.plugins_toml = std::env::var("PLUGINS_TOML")
            .map(PathBuf::from)
            .unwrap_or_else(|_| self.data_path.join("plugins.toml"));
        self.plugins_dir = std::env::var("PLUGINS_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| self.data_path.join("plugins"));
        std::fs::create_dir_all(&self.plugins_dir).unwrap_or_else(|e| {
            warn!(
                "failed to create plugins dir `{}`: {e}",
                self.plugins_dir.display()
            );
        });

        self.settings_plugin_path = std::env::var("SETTINGS_PLUGIN_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                std::env::current_exe()
                    .ok()
                    .and_then(|exe| exe.parent().map(|p| p.to_path_buf()))
                    .map(|dir| dir.join("settings"))
                    .unwrap_or_else(|| self.data_path.join("settings"))
            });
        self.feed_settings_plugin_path = std::env::var("FEED_SETTINGS_PLUGIN_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                std::env::current_exe()
                    .ok()
                    .and_then(|exe| exe.parent().map(|p| p.to_path_buf()))
                    .map(|dir| dir.join("feed-settings"))
                    .unwrap_or_else(|| self.data_path.join("feed-settings"))
            });
        self.voice_settings_plugin_path = std::env::var("VOICE_SETTINGS_PLUGIN_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                std::env::current_exe()
                    .ok()
                    .and_then(|exe| exe.parent().map(|p| p.to_path_buf()))
                    .map(|dir| dir.join("voice-settings"))
                    .unwrap_or_else(|| self.data_path.join("voice-settings"))
            });
        self.core_plugins = vec![
            CorePluginSpec {
                name: "settings".to_string(),
                path: self.settings_plugin_path.clone(),
            },
            CorePluginSpec {
                name: "feed-settings".to_string(),
                path: self.feed_settings_plugin_path.clone(),
            },
            CorePluginSpec {
                name: "voice-settings".to_string(),
                path: self.voice_settings_plugin_path.clone(),
            },
        ];

        self.features = Features {
            voice_tracking: parse_bool_env("ENABLE_VOICE_TRACKING", true),
            feed_publisher: parse_bool_env("ENABLE_FEED_PUBLISHER", true),
            autoregister_cmds: parse_bool_env("ENABLE_AUTOREGISTER_CMD", true),
        };

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

/// Parse boolean from environment variable.
/// Accepts: "true", "1", "yes", "on" (case-insensitive) as true.
fn parse_bool_env(var: &str, default: bool) -> bool {
    std::env::var(var)
        .ok()
        .and_then(|v| {
            let v = v.to_lowercase();
            match v.as_str() {
                "true" | "1" | "yes" | "on" => Some(true),
                "false" | "0" | "no" | "off" => Some(false),
                _ => None,
            }
        })
        .unwrap_or(default)
}
