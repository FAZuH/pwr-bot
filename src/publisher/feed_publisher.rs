use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;

use log::debug;
use log::error;
use log::info;

use crate::database::database::Database;
use crate::database::model::FeedItemModel;
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
            // Skip feeds with no subscribers
            let subs = self
                .db
                .feed_subscription_table
                .exists_by_feed_id(feed.id)
                .await?;

            if !subs {
                debug!("No subscriptions for feed {}. Skipping.", feed.name);
                continue;
            }

            // Get the latest known version for this feed
            let prev_version = match self
                .db
                .feed_item_table
                .select_latest_by_feed_id(feed.id)
                .await
            {
                Ok(version) => version,
                Err(_) => {
                    debug!(
                        "No previous version for feed {}. Will treat as new.",
                        feed.name
                    );
                    // Continue anyway - we'll create the first version if there's an update
                    continue;
                }
            };

            let source = match self.sources.get_feed_by_url(&feed.url) {
                Some(source) => source,
                None => {
                    // NOTE: This shouldn't happen
                    error!("Invalid url from db {}", feed.url);
                    continue;
                }
            };

            // Fetch current state from source
            let curr_check = match source.get_latest(&feed.id.to_string()).await {
                Ok(series) => series,
                Err(e) => {
                    if matches!(e, SeriesError::FinishedSeries { .. }) {
                        info!("Feed {} is finished. Removing from database.", feed.name);
                        self.db.feed_table.delete(&feed.id).await?;
                    } else {
                        error!("Error fetching feed {}: {}", feed.name, e);
                    }
                    continue;
                }
            };

            debug!(
                "Current version for feed {}: {}",
                feed.name, curr_check.latest
            );

            // Check if version changed
            if curr_check.latest == prev_version.description {
                debug!("No new version for feed {}.", feed.name);
                continue;
            }

            info!(
                "New version found for feed {}: {} -> {}",
                feed.name, prev_version.description, curr_check.latest
            );

            // Insert new version into database
            let new_version = FeedItemModel {
                id: 0, // Will be set by database
                feed_id: feed.id,
                description: curr_check.latest.clone(),
                published: curr_check.published,
            };
            let version_id = self.db.feed_item_table.replace(&new_version).await?;

            let feed_info = match source.get_info(&feed.id.to_string()).await {
                Ok(series) => series,
                Err(_) => {
                    // NOTE: This shouldn't happen
                    continue;
                }
            };

            // Publish update event
            info!("Publishing update event for feed {}.", feed.name);
            let event = FeedUpdateEvent {
                feed_id: feed.id,
                description: feed_info.description,
                version_id,
                title: feed_info.title,
                previous_version: prev_version.description,
                current_version: curr_check.latest,
                url: feed.url,
                published: curr_check.published,
            };
            self.event_bus.publish(event);
        }

        debug!("Finished checking for feed updates.");
        Ok(())
    }
}
