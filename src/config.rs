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
}

impl Config {
    /// Creates a new empty configuration.
    pub fn new() -> Self {
        Self {
            ..Default::default()
        }
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
        Ok(())
    }

    /// Gets a directory path from environment variable, creating it if needed.
    fn get_dirpath_mustexist(
        &self,
        var: &'static str,
        default: &'static str,
    ) -> Result<PathBuf, AppError> {
        #[allow(unused_must_use)]
        let val = std::env::var(var).unwrap_or(default.to_string());
        let path = PathBuf::from(val);
        let path_str = path.to_string_lossy();
        if !path.exists() {
            info!("Directory {path_str} does not exist. Creating...");
            let _ = std::fs::create_dir_all(&path);
        } else if !path.is_dir() {
            return Err(AppError::ConfigurationError {
                msg: format!("Path {path_str} exist but is a file when it must be a directory."),
            });
        };
        Ok(path)
    }
}
