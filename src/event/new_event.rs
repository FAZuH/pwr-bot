use crate::event::{NewChapterEvent, NewEpisodeEvent};

pub enum NewEvent {
    NewChapterEvent(NewChapterEvent),
    NewEpisodeEvent(NewEpisodeEvent)
}