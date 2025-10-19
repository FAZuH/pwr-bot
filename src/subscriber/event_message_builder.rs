use serenity::all::{Colour, CreateEmbed, CreateMessage, MessageFlags};

use crate::event::feed_update_event::FeedUpdateEvent;

pub struct EventMessageBuilder<'a> {
    event: &'a FeedUpdateEvent,
}

impl<'a> EventMessageBuilder<'a> {
    pub fn new(event: &'a FeedUpdateEvent) -> Self {
        Self { event }
    }

    pub fn build(&self) -> CreateMessage {
        let e = self.event;

        let title = format!("[{}]({})", e.title, e.url);
        let desc = format!(
            "- Old: {}\n- New: {}",
            e.previous_version, e.current_version
        );
        let embed = CreateEmbed::new()
            .colour(Colour::DARKER_GREY)
            .description(desc)
            .title(title);

        CreateMessage::new()
            .embed(embed)
            .flags(MessageFlags::SUPPRESS_EMBEDS)
    }
}
