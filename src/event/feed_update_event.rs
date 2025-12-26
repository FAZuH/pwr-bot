use serenity::all::CreateMessage;

use super::Event;
use crate::database::model::FeedModel;

/// Event fired when a new version/episode of a feed is published.
///
/// Contains both the previous and current version identifiers to enable
/// delta notifications (e.g., "Updated from Chapter 50 to Chapter 51").
#[derive(Clone, Debug)]
pub struct FeedUpdateEvent {
    pub feed: FeedModel,
    pub message: CreateMessage<'static>,
}

impl Event for FeedUpdateEvent {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
