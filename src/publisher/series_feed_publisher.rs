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

use crate::database::Database;
use crate::database::error::DatabaseError;
use crate::database::model::FeedItemModel;
use crate::database::model::FeedModel;
use crate::database::table::Table;
use crate::event::event_bus::EventBus;
use crate::event::feed_update_event::FeedUpdateEvent;
use crate::feed::FeedInfo;
use crate::feed::error::SeriesFeedError;
use crate::feed::feeds::Feeds;

pub struct SeriesFeedPublisher {
    db: Arc<Database>,
    event_bus: Arc<EventBus>,
    feeds: Arc<Feeds>,
    poll_interval: Duration,
    running: AtomicBool,
}

impl SeriesFeedPublisher {
    pub fn new(
        db: Arc<Database>,
        event_bus: Arc<EventBus>,
        feeds: Arc<Feeds>,
        poll_interval: Duration,
    ) -> Arc<Self> {
        info!(
            "Initializing FeedPublisher with poll interval {:?}",
            poll_interval
        );
        Arc::new(Self {
            db,
            event_bus,
            feeds,
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
        let feeds = self.db.feed_table.select_all_by_tag("series").await?;
        info!("Found {} feeds to check.", feeds.len());

        for feed in feeds {
            let id = feed.id;
            let name = feed.name.clone();
            if let Err(e) = self.check_feed(feed).await {
                error!("Error checking feed id `{id}` ({name}): {e:?}");
            };
        }

        debug!("Finished checking for feed updates.");
        Ok(())
    }

    async fn check_feed(&self, feed: FeedModel) -> anyhow::Result<()> {
        // Skip feeds with no subscribers
        let subs = self
            .db
            .feed_subscription_table
            .exists_by_feed_id(feed.id)
            .await?;

        if !subs {
            debug!(
                "No subscriptions for {}. Skipping.",
                self.get_feed_desc(&feed)
            );
            return Ok(());
        }

        // Get the latest known version for this feed
        let old_latest = self
            .db
            .feed_item_table
            .select_latest_by_feed_id(feed.id)
            .await?;

        let series_feed = self.feeds.get_feed_by_url(&feed.url).ok_or_else(|| {
            DatabaseError::InternalError {
                message: format!("Series feed source with url {} not found.", feed.url),
            }
            // NOTE: This means an invalid URL has been inserted to db due to insufficient
            // checks
        })?;

        let series_id = self.feeds.get_feed_id_by_url(&feed.url)?;
        // NOTE: Should've been checked already in commands.rs

        // Fetch current state from source
        let new_latest = match series_feed.get_latest(series_id).await {
            Ok(series) => series,
            Err(e) => {
                if matches!(e, SeriesFeedError::FinishedSeries { .. }) {
                    info!(
                        "Feed {} is finished. Removing from database.",
                        self.get_feed_desc(&feed)
                    );
                    self.db.feed_table.delete(&feed.id).await?;
                } else {
                    error!("Error fetching {}: {}", self.get_feed_desc(&feed), e);
                    return Err(e.into());
                }
                return Ok(());
            }
        };

        debug!(
            "Current version for {}: {}",
            self.get_feed_desc(&feed),
            new_latest.latest
        );

        // Check if version changed
        if new_latest.latest == old_latest.description {
            debug!("No new version for {}.", self.get_feed_desc(&feed));
            return Ok(());
        }
        info!(
            "New version found for {}: {} -> {}",
            self.get_feed_desc(&feed),
            old_latest.description,
            new_latest.latest
        );

        // Insert new version into database
        let new_feed_item = FeedItemModel {
            id: 0, // Will be set by database
            feed_id: feed.id,
            description: new_latest.latest.clone(),
            published: new_latest.published,
        };
        self.db.feed_item_table.replace(&new_feed_item).await?;

        // Set vars
        let message = self.create_message(
            &feed,
            &series_feed.get_base().info,
            &old_latest,
            &new_feed_item,
        );

        // Publish update event
        info!("Publishing update event for {}.", self.get_feed_desc(&feed));
        let event = FeedUpdateEvent { feed, message };
        self.event_bus.publish(event);
        Ok(())
    }

    fn get_feed_desc(&self, feed: &FeedModel) -> String {
        format!("feed id `{}` ({})", feed.id, feed.name)
    }

    fn create_message(
        &self,
        feed: &FeedModel,
        feed_info: &FeedInfo,
        old_feed_item: &FeedItemModel,
        new_feed_item: &FeedItemModel,
    ) -> CreateMessage<'static> {
        let feed_desc = feed
            .description
            .trim_start()
            .trim_end()
            .replace("\n", "\n> ")
            .replace("<br>", "");
        let text_main = format!(
            "### {}

> {}

**Old {}**: {}
Published on <t:{}>

**New {}**: {}
Published on <t:{}>

**[Open in browser ↗]({})**",
            feed.name,
            feed_desc,
            feed_info.feed_type,
            old_feed_item.description,
            old_feed_item.published.timestamp(),
            feed_info.feed_type,
            new_feed_item.description,
            new_feed_item.published.timestamp(),
            feed.url
        );
        let text_footer = format!("-# {}", feed_info.copyright_notice);

        let container = CreateContainer::new(vec![
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
        ]);

        CreateMessage::new()
            .flags(MessageFlags::IS_COMPONENTS_V2)
            .components(vec![CreateComponent::Container(container)])
    }
}
