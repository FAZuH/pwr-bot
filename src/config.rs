#[derive(Clone, Default)]
pub struct Config {
    pub poll_interval: u64,
    pub db_url: String,
    pub db_path: String,
    pub discord_token: String,
    pub webhook_url: String,
}

impl Config {
    pub fn new() -> Self {
        Self {
            poll_interval: std::env::var("POLL_INTERVAL")
                .unwrap_or("180".to_string())
                .parse::<u64>()
                .unwrap_or(180),
            db_url: std::env::var("DB_URL").unwrap_or("sqlite://data.db".to_string()),
            db_path: std::env::var("DB_PATH").unwrap_or("data.db".to_string()),
            discord_token: std::env::var("DISCORD_TOKEN")
                .expect("Expected DISCORD_TOKEN in environment"),
            webhook_url: std::env::var("WEBHOOK_URL").expect("Expected WEBHOOK_URL in environment"),
        }
    }
}
