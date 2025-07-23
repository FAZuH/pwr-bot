use std::any::Any;
use super::event::Event;

#[derive(Clone)]
pub struct AnimeUpdateEvent {
    pub series_id: String,
    pub series_type: String,
    pub title: String,
    pub chapter: String,
    pub chapter_id: String,
    pub url: String,
}

impl Event for AnimeUpdateEvent {
    fn as_any(&self) -> &dyn Any {
        self
    }
}
