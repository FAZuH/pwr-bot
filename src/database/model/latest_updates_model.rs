use chrono::{DateTime, Utc};
use sqlx::FromRow;

#[derive(FromRow)]
pub struct LatestUpdatesModel {
    pub id: u32,
    pub r#type: String,         // Anime/Manga
    pub series_id: String,      // Series identifier eg Frieren
    pub series_latest: String,  // Latest of series identifer eg S2E1
    pub series_published: DateTime<Utc> }
