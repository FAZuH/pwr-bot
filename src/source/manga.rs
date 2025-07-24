use chrono::{DateTime, Utc};

#[derive(Clone)]
pub struct Manga {
    pub series_id: String,
    pub series_type: String,
    pub title: String,
    pub chapter: String,
    pub chapter_id: String,
    pub url: String,
    pub published: DateTime<Utc>
}
