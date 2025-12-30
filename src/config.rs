use std::time::Duration;

use crate::error::AppError;

#[derive(Clone, Default)]
pub struct Config {
    pub poll_interval: Duration,
    pub db_url: String,
    pub db_path: String,
    pub discord_token: String,
    pub admin_id: String,
    pub logs_path: String,
}

impl Config {
    pub fn new() -> Self {
        Self {
            ..Default::default()
        }
    }

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
        self.logs_path = std::env::var("LOGS_PATH").unwrap_or("./logs".to_string());
        Ok(())
    }
}
