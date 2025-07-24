use std::any::Any;
use chrono::{DateTime, Utc};

use crate::source::anime::Anime;

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

impl From<Anime> for AnimeUpdateEvent {
    fn from(anime: Anime) -> Self {
        AnimeUpdateEvent {
            series_id: anime.series_id,
            series_type: anime.series_type,
            title: anime.title,
            chapter: anime.chapter,
            chapter_id: anime.chapter_id,
            url: anime.url,
            published: anime.published,
        }
    }
}

impl Event for AnimeUpdateEvent {
    fn as_any(&self) -> &dyn Any {
        self
    }
}
