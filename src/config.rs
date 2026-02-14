//! Configuration management for the bot.
//!
//! Handles loading configuration from environment variables.

use std::path::PathBuf;
use std::time::Duration;

use log::info;

use crate::error::AppError;

/// Bot configuration loaded from environment variables.
#[derive(Clone, Default)]
pub struct Config {
    pub poll_interval: Duration,
    pub db_url: String,
    pub db_path: String,
    pub discord_token: String,
    pub admin_id: String,
    pub data_path: PathBuf,
    pub logs_path: PathBuf,
    pub features: Features,
}

/// Feature flags for optional bot components.
#[derive(Clone, Default)]
pub struct Features {
    pub voice_tracking: bool,
    pub feed_publisher: bool,
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

        self.db_url = std::env::var("DATABASE_URL").unwrap_or("sqlite://data.db".to_string());
        self.db_path = std::env::var("DATABASE_PATH").unwrap_or("./data/data.db".to_string());

        self.discord_token =
            std::env::var("DISCORD_TOKEN").map_err(|_| AppError::MissingConfig {
                config: "DISCORD_TOKEN".to_string(),
            })?;

        self.admin_id = std::env::var("ADMIN_ID").map_err(|_| AppError::MissingConfig {
            config: "ADMIN_ID".to_string(),
        })?;

        self.data_path = self.get_dirpath_mustexist("DATA_PATH", "./data")?;
        self.logs_path = self.get_dirpath_mustexist("LOGS_PATH", "./logs")?;

        self.features = Features {
            voice_tracking: parse_bool_env("ENABLE_VOICE_TRACKING", true),
            feed_publisher: parse_bool_env("ENABLE_FEED_PUBLISHER", true),
        };

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
