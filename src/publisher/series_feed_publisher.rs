use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;

use log::debug;
use log::error;
use log::info;
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
use tokio::time::Sleep;
use tokio::time::sleep;

use crate::database::model::FeedItemModel;
use crate::database::model::FeedModel;
use crate::event::FeedUpdateEvent;
use crate::event::event_bus::EventBus;
use crate::feed::PlatformInfo;
use crate::service::feed_subscription_service::FeedSubscriptionService;
use crate::service::feed_subscription_service::FeedUpdateResult;

pub struct SeriesFeedPublisher {
    service: Arc<FeedSubscriptionService>,
    event_bus: Arc<EventBus>,
    poll_interval: Duration,
    running: AtomicBool,
}

impl SeriesFeedPublisher {
    pub fn new(
        service: Arc<FeedSubscriptionService>,
        event_bus: Arc<EventBus>,
        poll_interval: Duration,
    ) -> Arc<Self> {
        info!(
            "Initializing FeedPublisher with poll interval {:?}",
            poll_interval
        );
        Arc::new(Self {
            service,
            event_bus,
            poll_interval,
            running: AtomicBool::new(false),
        })
    }

    pub fn start(self: Arc<Self>) -> anyhow::Result<()> {
        if !self.running.load(Ordering::SeqCst) {
            self.running.store(true, Ordering::SeqCst);
            info!("Starting FeedPublisher check loop.");
            self.spawn_check_loop();
        }
        Ok(())
    }

    pub fn stop(self: Arc<Self>) -> anyhow::Result<()> {
        info!("Stopping FeedPublisher check loop.");
        self.running.store(false, Ordering::SeqCst);
        Ok(())
    }

    fn spawn_check_loop(self: Arc<Self>) {
        let mut interval = tokio::time::interval(self.poll_interval);
        tokio::spawn(async move {
            loop {
                interval.tick().await;
                if !self.running.load(Ordering::SeqCst) {
                    info!("Stopping check loop.");
                    break;
                }
                if let Err(e) = self.check_updates().await {
                    error!("Error checking updates: {}", e);
                }
            }
        });
    }

    async fn check_updates(&self) -> anyhow::Result<()> {
        debug!("Checking for feed updates.");

        // Get all feeds containing tag "series"
        let feeds = self.service.get_feeds_by_tag("series").await?;
        let feeds_len = feeds.len();
        info!("Found {} feeds to check.", feeds.len());

        for feed in feeds {
            let id = feed.id;
            let name = feed.name.clone();
            if let Err(e) = self.check_feed(feed).await {
                error!("Error checking feed id `{id}` ({name}): {e:?}");
            };
            Self::check_feed_wait(feeds_len, &self.poll_interval).await;
        }

        debug!("Finished checking for feed updates.");
        Ok(())
    }

    async fn check_feed(&self, feed: FeedModel) -> anyhow::Result<()> {
        match self.service.check_feed_update(&feed).await? {
            FeedUpdateResult::NoUpdate => {
                debug!(
                    "No update or no subscribers for {}.",
                    self.get_feed_desc(&feed)
                );
                Ok(())
            }
            FeedUpdateResult::SourceFinished => {
                info!(
                    "Feed {} is finished. Removed from database.",
                    self.get_feed_desc(&feed)
                );
                Ok(())
            }
            FeedUpdateResult::Updated {
                feed: _,
                old_item,
                new_item,
                feed_info,
            } => {
                info!(
                    "New version found for {}: {} -> {}",
                    self.get_feed_desc(&feed),
                    old_item
                        .as_ref()
                        .map_or("None".to_string(), |e| e.description.clone()),
                    new_item.description
                );

                // Set vars
                let message = self.create_message(&feed, &feed_info, old_item.as_ref(), &new_item);

                // Publish update event
                info!("Publishing update event for {}.", self.get_feed_desc(&feed));
                let event = FeedUpdateEvent { feed, message };
                self.event_bus.publish(event);
                Ok(())
            }
        }
    }

    fn get_feed_desc(&self, feed: &FeedModel) -> String {
        format!("feed id `{}` ({})", feed.id, feed.name)
    }

    /// Insipred by freestuffbot.xyz's notifications
    fn create_message(
        &self,
        feed: &FeedModel,
        feed_info: &PlatformInfo,
        old_feed_item: Option<&FeedItemModel>,
        new_feed_item: &FeedItemModel,
    ) -> CreateMessage<'static> {
        let title =
            CreateComponent::TextDisplay(CreateTextDisplay::new(format!("### {}", feed.name)));

        let feed_desc = feed
            .description
            .trim_start()
            .trim_end()
            .replace("\n", "\n> ")
            .replace("<br>", "");
        let feed_desc: String = html2md::parse_html(&feed_desc)
            // Prevent panic from splitting in the middle of a multi-byte UTF-8 character
            .chars()
            .take(500)
            .collect();

        let old_section = old_feed_item.map_or(
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
            "> {}

{}

**New {}**: {}
Published on <t:{}>

**[Open in browser ↗]({})**",
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
            .components(vec![title, container])
    }

    fn check_feed_wait(feeds_length: usize, poll_interval: &Duration) -> Sleep {
        sleep(Self::calculate_feed_interval(feeds_length, poll_interval))
    }

    fn calculate_feed_interval(feeds_length: usize, poll_interval: &Duration) -> Duration {
        let feeds_count = feeds_length.max(1) as u64;
        Duration::from_millis(poll_interval.as_millis() as u64 / feeds_count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_feed_interval_calculation() {
        assert_eq!(
            SeriesFeedPublisher::calculate_feed_interval(10, &Duration::from_secs(60)),
            Duration::from_secs(6)
        );

        assert_eq!(
            SeriesFeedPublisher::calculate_feed_interval(0, &Duration::from_secs(60)),
            Duration::from_secs(60) // Division by 1 when length is 0
        );
    }
}
