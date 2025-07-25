use chrono::{DateTime, Utc};

#[derive(Clone)]
pub struct Anime {
    pub series_id: String,
    pub series_type: String,
    pub title: String,
    pub episode: String,
    pub url: String,
    pub published: DateTime<Utc>,
}
