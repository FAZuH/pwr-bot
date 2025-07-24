use crate::source::manga::Manga;
use chrono::{DateTime, Utc};

use std::any::Any;
use super::event::Event;

#[derive(Clone)]
pub struct MangaUpdateEvent {
    pub series_id: String,
    pub series_type: String,
    pub title: String,
    pub chapter: String,
    pub chapter_id: String,
    pub url: String,
    pub published: DateTime<Utc>
}

impl From<Manga> for MangaUpdateEvent {
    fn from(manga: Manga) -> Self {
        MangaUpdateEvent {
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

impl Event for MangaUpdateEvent {
    fn as_any(&self) -> &dyn Any {
        self
    }
}
