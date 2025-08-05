use chrono::{DateTime, Utc};
use std::any::Any;

use super::Event;

#[derive(Clone, Debug)]
pub struct SeriesUpdateEvent {
    pub latest_results_id: u32,
    pub title: String,
    pub previous: String,
    pub current: String,
    pub url: String,
    pub published: DateTime<Utc>,
}

impl Event for SeriesUpdateEvent {
    fn as_any(&self) -> &dyn Any {
        self
    }
}
