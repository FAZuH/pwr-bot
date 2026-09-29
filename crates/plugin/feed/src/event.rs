//! Feed update event and notification message creation.

use std::any::Any;
use std::any::TypeId;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::RwLock;

use anyhow::Result;
use pwr_ext::view_support::CreateComponent;
use pwr_ext::view_support::CreateContainer;
use pwr_ext::view_support::CreateContainerComponent;
use pwr_ext::view_support::CreateMediaGallery;
use pwr_ext::view_support::CreateMediaGalleryItem;
use pwr_ext::view_support::CreateMessage;
use pwr_ext::view_support::CreateSection;
use pwr_ext::view_support::CreateSectionAccessory;
use pwr_ext::view_support::CreateSectionComponent;
use pwr_ext::view_support::CreateSeparator;
use pwr_ext::view_support::CreateTextDisplay;
use pwr_ext::view_support::CreateThumbnail;
use pwr_ext::view_support::CreateUnfurledMediaItem;
use pwr_ext::view_support::MessageFlags;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

use crate::PlatformInfo;
use crate::entity::FeedEntity;
use crate::entity::FeedItemEntity;
use crate::subscriber::Subscriber;

type AsyncSubscriber<E> =
    Box<dyn Fn(E) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> + Send + Sync>;
type Subscribers = Arc<RwLock<HashMap<TypeId, Vec<Box<dyn Any + Send + Sync>>>>>;

pub struct EventBus {
    subscribers: Subscribers,
}

impl EventBus {
    pub fn new() -> Self {
        Self {
            subscribers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn register_callback<E, F, Fut>(&self, callback: F) -> &Self
    where
        E: 'static + Send + Sync,
        F: Fn(E) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        let callback: AsyncSubscriber<E> = Box::new(move |event| Box::pin(callback(event)));
        self.subscribers
            .write()
            .unwrap()
            .entry(TypeId::of::<E>())
            .or_default()
            .push(Box::new(callback));
        self
    }

    pub fn register_subscriber<E, S>(&self, subscriber: Arc<S>) -> &Self
    where
        E: 'static + Send + Sync + Clone,
        S: Subscriber<E> + Send + Sync + 'static,
    {
        self.register_callback(move |event| {
            let subscriber = subscriber.clone();
            async move { subscriber.callback(event).await }
        })
    }

    pub fn publish<E>(&self, event: E)
    where
        E: 'static + Send + Sync + Clone,
    {
        let subscribers = self.subscribers.read().unwrap();
        let Some(subscribers) = subscribers.get(&TypeId::of::<E>()) else {
            return;
        };
        let mut futures = Vec::new();
        for subscriber in subscribers {
            if let Some(subscriber) = subscriber.downcast_ref::<AsyncSubscriber<E>>() {
                futures.push(subscriber(event.clone()));
            }
        }
        tokio::spawn(async move {
            futures::future::join_all(futures).await;
        });
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

/// Event fired when a new version/episode of a feed is published.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FeedUpdateEvent {
    pub feed: Arc<FeedEntity>,
    pub data: Arc<FeedUpdateData>,
}

impl FeedUpdateEvent {
    /// Creates a new feed update event from the given data.
    pub fn new(data: FeedUpdateData) -> Self {
        Self {
            feed: data.feed.clone(),
            data: Arc::new(data),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FeedUpdateData {
    pub feed: Arc<FeedEntity>,
    pub feed_info: Arc<PlatformInfo>,
    pub old_feed_item: Option<Arc<FeedItemEntity>>,
    pub new_feed_item: Arc<FeedItemEntity>,
}

impl FeedUpdateData {
    /// Creates a Discord message for this feed update.
    pub fn create_message(&self) -> Value {
        let feed_desc = if self.feed.description.is_empty() {
            "> No description.".to_string()
        } else {
            let mut description = html2md::parse_html(&self.feed.description)
                .trim()
                .replace(r"\*", "*")
                .replace(r"\_", "_")
                .replace(r"\[", "[")
                .replace(r"\]", "]")
                .replace(r"\(", "(")
                .replace(r"\)", ")")
                .replace("\n\n", "\n")
                .replace('\n', "\n> \n> ");
            description.insert_str(0, "> ");
            if description.len() > 500 {
                description = description
                    .chars()
                    .take(500)
                    .collect::<String>()
                    .trim_end_matches('\n')
                    .to_string();
                description.push_str("\n> ...");
            }
            description
        };

        let old_section = self.old_feed_item.as_ref().map_or_else(
            || format!("**No previous {} **", self.feed_info.feed_item_name),
            |old| {
                format!(
                    "**Old {}**: {}\nPublished on <t:{}>",
                    self.feed_info.feed_item_name,
                    old.description,
                    old.published.timestamp()
                )
            },
        );
        let text_main = format!(
            concat!(
                "### {}\n\n{}\n\n{}\n\n**New {}**: {}\nPublished on <t:{}>\n\n",
                "**[Open in browser ↗]({})**"
            ),
            self.feed.name,
            feed_desc,
            old_section,
            self.feed_info.feed_item_name,
            self.new_feed_item.description,
            self.new_feed_item.published.timestamp(),
            self.feed.source_url
        );
        let text_footer = format!("-# {}", self.feed_info.copyright_notice);
        let container = CreateContainer::new(vec![
            CreateContainerComponent::Section(CreateSection::new(
                vec![CreateSectionComponent::TextDisplay(CreateTextDisplay::new(
                    text_main,
                ))],
                CreateSectionAccessory::Thumbnail(CreateThumbnail::new(
                    CreateUnfurledMediaItem::new(self.feed_info.logo_url.clone()),
                )),
            )),
            CreateContainerComponent::Separator(CreateSeparator::new().divider(false)),
            CreateContainerComponent::MediaGallery(CreateMediaGallery::new(vec![
                CreateMediaGalleryItem::new(CreateUnfurledMediaItem::new(
                    self.feed.cover_url.clone(),
                )),
            ])),
            CreateContainerComponent::TextDisplay(CreateTextDisplay::new(text_footer)),
        ]);
        let message = CreateMessage::new()
            .flags(MessageFlags::IS_COMPONENTS_V2)
            .components(vec![CreateComponent::Container(container)]);
        serde_json::to_value(message).expect("feed update message is serializable")
    }
}
