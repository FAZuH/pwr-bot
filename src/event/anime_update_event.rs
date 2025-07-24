use std::any::Any;
use chrono::{DateTime, Utc};
use crate::source::manga::Manga;

use super::event::Event;

#[derive(Clone)]
pub struct AnimeUpdateEvent {
    pub series_id: String,
    pub series_type: String,
    pub title: String,
    pub chapter: String,
    pub chapter_id: String,
    pub url: String,
    pub published: DateTime<Utc>
}

impl From<Manga> for AnimeUpdateEvent {
    fn from(manga: Manga) -> Self {
        AnimeUpdateEvent {
            series_id: manga.series_id,
            series_type: manga.series_type,
            title: manga.title,
            chapter: manga.chapter,
            chapter_id: manga.chapter_id,
            url: manga.url,
            published: manga.published,
        }
    }
}
impl Event for AnimeUpdateEvent {
    fn as_any(&self) -> &dyn Any {
        self
    }
}
