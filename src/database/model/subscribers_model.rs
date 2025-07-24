use sqlx::FromRow;

#[derive(FromRow)]
pub struct SubscribersModel {
    pub id: u32,
    pub subscriber_type: String,    // Webhook/DM
    pub subscriber_id: String,      // Webhook URL/User ID
    pub latest_updates_id: u32      // Foreign key to LatestUpdateModel
}
