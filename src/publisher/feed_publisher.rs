use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;

use log::debug;
use log::error;
use log::info;

use crate::database::database::Database;
use crate::database::error::DatabaseError;
use crate::database::model::FeedItemModel;
use crate::database::model::FeedModel;
use crate::database::table::Table;
use crate::event::event_bus::EventBus;
use crate::event::feed_update_event::FeedUpdateEvent;
use crate::feed::error::SeriesError;
use crate::feed::feeds::Feeds;

pub struct FeedPublisher {
    db: Arc<Database>,
    event_bus: Arc<EventBus>,
    sources: Arc<Feeds>,
    poll_interval: Duration,
    running: AtomicBool,
}

impl FeedPublisher {
    pub fn new(
        db: Arc<Database>,
        event_bus: Arc<EventBus>,
        sources: Arc<Feeds>,
        poll_interval: Duration,
    ) -> Arc<Self> {
        info!(
            "Initializing FeedPublisher with poll interval {:?}",
            poll_interval
        );
        Arc::new(Self {
            db,
            event_bus,
            sources,
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
            if let Err(e) = self.check_feed(&feed).await {
                error!(
                    "Error checking feed with id {} ({}): {:?}",
                    feed.id, feed.name, e
                );
            };
        }

        debug!("Finished checking for feed updates.");
        Ok(())
    }

    async fn check_feed(&self, feed: &FeedModel) -> anyhow::Result<()> {
        // Skip feeds with no subscribers
        let subs = self
            .db
            .feed_subscription_table
            .exists_by_feed_id(feed.id)
            .await?;

        if !subs {
            debug!(
                "No subscriptions for {}. Skipping.",
                self.get_feed_desc(feed)
            );
            return Ok(());
        }

        // Get the latest known version for this feed
        let old_latest = self
            .db
            .feed_item_table
            .select_latest_by_feed_id(feed.id)
            .await?;

        let series_feed = self.sources.get_feed_by_url(&feed.url).ok_or_else(|| {
            DatabaseError::InternalError {
                message: format!("Series feed source with url {} not found.", feed.url),
            }
            // NOTE: This means an invalid URL has been inserted to db due to insufficient
            // checks
        })?;

        let series_id = self.sources.get_feed_id_by_url(&feed.url)?;
        // NOTE: Should've been checked already in commands.rs

        // Fetch current state from source
        let curr_latest = match series_feed.get_latest(series_id).await {
            Ok(series) => series,
            Err(e) => {
                if matches!(e, SeriesError::FinishedSeries { .. }) {
                    info!(
                        "Feed {} is finished. Removing from database.",
                        self.get_feed_desc(feed)
                    );
                    self.db.feed_table.delete(&feed.id).await?;
                } else {
                    error!("Error fetching {}: {}", self.get_feed_desc(feed), e);
                    return Err(e.into());
                }
                return Ok(());
            }
        };

        debug!(
            "Current version for {}: {}",
            self.get_feed_desc(feed),
            curr_latest.latest
        );

        // Check if version changed
        if curr_latest.latest == old_latest.description {
            debug!("No new version for {}.", self.get_feed_desc(feed));
            return Ok(());
        }
        info!(
            "New version found for {}: {} -> {}",
            self.get_feed_desc(feed),
            old_latest.description,
            curr_latest.latest
        );

        // Insert new version into database
        let new_version = FeedItemModel {
            id: 0, // Will be set by database
            feed_id: feed.id,
            description: curr_latest.latest.clone(),
            published: curr_latest.published,
        };
        let version_id = self.db.feed_item_table.replace(&new_version).await?;

        let feed_info = series_feed.get_info(series_id).await?;

        // Publish update event
        info!(
            "Publishing update event for {}.",
            self.get_feed_desc(feed)
        );
        let event = FeedUpdateEvent {
            feed_id: feed.id,
            description: feed_info.description,
            version_id,
            title: feed_info.title,
            previous_version: old_latest.description,
            current_version: curr_latest.latest,
            url: feed.url.clone(),
            published: curr_latest.published,
        };
        self.event_bus.publish(event);
        Ok(())
    }

    fn get_feed_desc(&self, feed: &FeedModel) -> String {
        format!("feed id {} ({})", feed.id, feed.name)
    }
}
