use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::FromRow;

#[derive(FromRow, Debug, Serialize)]
pub struct LatestResultModel {
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

#[derive(FromRow, Debug, Serialize, Default)]
pub struct SubscribersModel {
    pub id: u32,
    pub r#type: String, // Webhook/DM
    pub target: String,   // Webhook URL/User ID
    pub latest_results_id: u32,  // Foreign key
}
