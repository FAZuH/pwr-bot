use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::FromRow;

#[derive(FromRow, Serialize)] pub struct LatestResultModel {
    pub id: u32,
    pub name: String,   // eg Frieren
    pub latest: String, // eg S2E1
    pub url: String,    // eg 12345
    pub tags: String,   // eg "series"
    pub published: DateTime<Utc>,
}

impl Default for LatestResultModel {
    fn default() -> Self {
        Self {
            id: 0,
            name: String::new(),
            latest: String::new(),
            url: String::new(),
            tags: String::new(),
            published: DateTime::<Utc>::MIN_UTC,
        }
    }
}

#[derive(FromRow, Serialize, Default)]
pub struct SubscribersModel {
    pub id: u32,
    pub r#type: String,         // Guild/DM
    pub target: String,         // Guild ID/User ID
    pub latest_results_id: u32, // Foreign key
}

// We need an additional field to store webhook URL for each guild
#[derive(FromRow, Serialize, Default)]
pub struct GuildNotifyTargets {
    pub guild_id: i64,          // Guild ID
    pub webhook_url: String,    // Webhook URL/User ID
}
