use chrono::{DateTime, Utc};
use std::any::Any;

use crate::source::anime::Anime;

use super::event::Event;

#[derive(Clone, Debug)]
pub struct AnimeUpdateEvent {
    pub series_id: String,
    pub series_type: String,
    pub title: String,
    pub episode: String,
    pub url: String,
    pub published: DateTime<Utc>,
}

impl From<Anime> for AnimeUpdateEvent {
    fn from(anime: Anime) -> Self {
        AnimeUpdateEvent {
            series_id: anime.series_id,
            series_type: anime.series_type,
            title: anime.title,
            episode: anime.episode,
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
