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
    /// Core plugins the host spawns at startup, in order.
    pub core_plugins: Vec<CorePluginSpec>,
    pub features: Features,
    pub version: String,
}

/// One core plugin the host spawns at startup: its name and binary path.
/// The set of core plugins is configuration, not source: `CORE_PLUGINS`
/// lists their names, and each binary's path resolves through
/// [`Config::core_plugin_path`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CorePluginSpec {
    /// Plugin name, e.g. `feed`.
    pub name: String,
    /// Binary path to spawn.
    pub path: PathBuf,
}

/// Feature flags for optional bot components.
#[derive(Clone, Default, Debug)]
pub struct Features {
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

        self.core_plugins = parse_core_plugins(&std::env::var("CORE_PLUGINS").unwrap_or_default())
            .into_iter()
            .map(|name| CorePluginSpec {
                path: self.core_plugin_path(&name),
                name,
            })
            .collect();

        self.features = Features {
            autoregister_cmds: parse_bool_env("ENABLE_AUTOREGISTER_CMD", true),
        };

        self.version = env!("CARGO_PKG_VERSION").to_string();

        Ok(())
    }

    /// Resolves a core plugin's binary path: the `<NAME>_PLUGIN_PATH` env
    /// var (the plugin's uppercased name) when set, else the binary shipped
    /// next to the bot binary, else one under the data path.
    fn core_plugin_path(&self, name: &str) -> PathBuf {
        std::env::var(format!("{}_PLUGIN_PATH", name.to_uppercase()))
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                std::env::current_exe()
                    .ok()
                    .and_then(|exe| exe.parent().map(|p| p.to_path_buf()))
                    .map(|dir| dir.join(name))
                    .unwrap_or_else(|| self.data_path.join(name))
            })
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

/// The core plugin names from a `CORE_PLUGINS` value: a comma-separated
/// list, trimmed, empty entries dropped.
fn parse_core_plugins(list: &str) -> Vec<String> {
    list.split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .collect()
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

#[cfg(test)]
mod tests {
    use super::parse_core_plugins;

    #[test]
    fn core_plugins_list_is_split_trimmed_and_empties_dropped() {
        assert_eq!(
            parse_core_plugins(" feed , voice ,,welcome"),
            vec![
                "feed".to_string(),
                "voice".to_string(),
                "welcome".to_string()
            ]
        );
        assert!(parse_core_plugins("").is_empty());
        assert!(parse_core_plugins(" , ,").is_empty());
    }
}
