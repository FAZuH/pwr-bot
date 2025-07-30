use serde::Serialize;
use chrono::{DateTime, Utc};
use sqlx::FromRow;

#[derive(FromRow, Debug, Serialize)]
pub struct LatestUpdatesModel {
    pub id: u32,
    pub r#type: String,        // Anime/Manga
    pub series_id: String,     // eg 12345
    pub series_title: String,  // eg Frieren
    pub series_latest: String, // eg S2E1
    pub series_published: DateTime<Utc>,
}

impl Default for LatestUpdatesModel {
    fn default() -> Self {
        Self {
            id: 0,
            r#type: String::new(),
            series_id: String::new(),
            series_title: String::new(),
            series_latest: String::new(),
            series_published: DateTime::<Utc>::MIN_UTC,
        }
    }
}
