//! Feed update event and notification message creation.

use std::sync::Arc;

use serenity::all::CreateComponent;
use serenity::all::CreateContainer;
use serenity::all::CreateContainerComponent;
use serenity::all::CreateMediaGallery;
use serenity::all::CreateMediaGalleryItem;
use serenity::all::CreateMessage;
use serenity::all::CreateSection;
use serenity::all::CreateSectionAccessory;
use serenity::all::CreateSectionComponent;
use serenity::all::CreateSeparator;
use serenity::all::CreateTextDisplay;
use serenity::all::CreateThumbnail;
use serenity::all::CreateUnfurledMediaItem;
use serenity::all::MessageFlags;

use crate::event::Event;
use crate::feed::PlatformInfo;
use crate::model::FeedItemModel;
use crate::model::FeedModel;

/// Event fired when a new version/episode of a feed is published.
#[derive(Clone, Debug)]
pub struct FeedUpdateEvent {
    pub feed: Arc<FeedModel>,
    pub data: Arc<FeedUpdateData>,
}

impl FeedUpdateEvent {
    /// Creates a new feed update event from the given data.
    pub fn new(data: FeedUpdateData) -> Self {
        let data = Arc::new(data);
        Self {
            feed: data.feed.clone(),
            data,
        }
    }
}

#[derive(Clone, Debug)]
pub struct FeedUpdateData {
    pub feed: Arc<FeedModel>,
    pub feed_info: Arc<PlatformInfo>,
    pub old_feed_item: Option<Arc<FeedItemModel>>,
    pub new_feed_item: Arc<FeedItemModel>,
}

impl FeedUpdateData {
    /// Creates a Discord message for this feed update.
    pub fn create_message(&self) -> CreateMessage<'static> {
        let FeedUpdateData {
            feed,
            feed_info,
            old_feed_item,
            new_feed_item,
        } = self;
        let feed_desc = if feed.description.is_empty() {
            "> No description.".to_string()
        } else {
            let mut desc = html2md::parse_html(&feed.description)
                .trim_start()
                .trim_end()
                .replace(r"\*", "*")
                .replace(r"\_", "_")
                .replace(r"\[", "[")
                .replace(r"\]", "]")
                .replace(r"\(", "(")
                .replace(r"\)", ")")
                .replace("\n\n", "\n")
                .replace("\n", "\n> \n> ")
                .trim_end_matches("\n")
                .to_string();
            desc.insert_str(0, "> ");

            if desc.len() > 500 {
                // Prevent panic from splitting in the middle of a multi-byte UTF-8 character
                desc = desc
                    .chars()
                    .take(500)
                    .collect::<String>()
                    .trim_end_matches("\n")
                    .to_string();
                desc.push_str("\n> ...");
            }

            desc
        };

        let old_section = old_feed_item.clone().map_or(
            format!("**No previous {} **", feed_info.feed_item_name),
            |old| {
                format!(
                    "**Old {}**: {}\nPublished on <t:{}>",
                    feed_info.feed_item_name,
                    old.description,
                    old.published.timestamp()
                )
            },
        );

        let text_main = format!(
            "### {}

{}

{}

**New {}**: {}
Published on <t:{}>

**[Open in browser ↗]({})**",
            feed.name,
            feed_desc,
            old_section,
            feed_info.feed_item_name,
            new_feed_item.description,
            new_feed_item.published.timestamp(),
            feed.source_url
        );
        let text_footer = format!("-# {}", feed_info.copyright_notice);

        let container = CreateComponent::Container(CreateContainer::new(vec![
            CreateContainerComponent::Section(CreateSection::new(
                vec![CreateSectionComponent::TextDisplay(CreateTextDisplay::new(
                    text_main,
                ))],
                CreateSectionAccessory::Thumbnail(CreateThumbnail::new(
                    CreateUnfurledMediaItem::new(feed_info.logo_url.clone()),
                )),
            )),
            CreateContainerComponent::Separator(CreateSeparator::new(false)),
            CreateContainerComponent::MediaGallery(CreateMediaGallery::new(vec![
                CreateMediaGalleryItem::new(CreateUnfurledMediaItem::new(feed.cover_url.clone())),
            ])),
            CreateContainerComponent::TextDisplay(CreateTextDisplay::new(text_footer)),
        ]));

        CreateMessage::new()
            .flags(MessageFlags::IS_COMPONENTS_V2)
            .components(vec![container])
    }
}

impl Event for FeedUpdateEvent {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
