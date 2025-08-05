use serde::Serialize;
use sqlx::FromRow;

#[derive(FromRow, Debug, Serialize)]
pub struct SubscribersModel {
    pub id: u32,
    pub subscriber_type: String, // Webhook/DM
    pub subscriber_id: String,   // Webhook URL/User ID
    pub latest_results_id: u32,  // Foreign key
}

impl Default for SubscribersModel {
    fn default() -> Self {
        Self {
            id: 0,
            subscriber_type: String::new(),
            subscriber_id: String::new(),
            latest_results_id: 0,
        }
    }
}
