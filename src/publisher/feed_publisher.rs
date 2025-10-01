use crate::database::database::Database;
use crate::database::table::Table;
use crate::event::event_bus::EventBus;
use crate::event::feed_update_event::FeedUpdateEvent;
use crate::source::error::SourceError;
use crate::source::model::SourceResult;
use crate::source::sources::Sources;
use log::{debug, error, info};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub struct FeedPublisher {
    db: Arc<Database>,
    event_bus: Arc<EventBus>,
    sources: Arc<Sources>,
    poll_interval: Duration,
    running: AtomicBool,
}

impl FeedPublisher {
    pub fn new(
        db: Arc<Database>,
        event_bus: Arc<EventBus>,
        sources: Arc<Sources>,
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
                debug!("FeedPublisher: Tick.");
                if !self.running.load(Ordering::SeqCst) {
                    info!("FeedPublisher: Stopping check loop.");
                    break;
                }
                if let Err(e) = self.check_updates().await {
                    error!("FeedPublisher: Error checking updates: {}", e);
                }
            }
        });
    }

    async fn check_updates(&self) -> anyhow::Result<()> {
        debug!("FeedPublisher: Checking for feed updates.");

        // Get all feeds tagged as "series"
        let feeds = self.db.feed_table.select_all_by_tag("series").await?;
        info!("FeedPublisher: Found {} feeds to check.", feeds.len());

        for feed in feeds {
            // Skip feeds with no subscriptions
            let subscriptions = self
                .db
                .feed_subscription_table
                .select_all_by_feed_id(feed.id)
                .await?;

            if subscriptions.is_empty() {
                debug!(
                    "FeedPublisher: No subscriptions for feed {}. Skipping.",
                    feed.name
                );
                continue;
            }

            // Get the latest known version for this feed
            let prev_version = match self
                .db
                .feed_version_table
                .select_latest_by_feed_id(feed.id)
                .await
            {
                Ok(version) => version,
                Err(_) => {
                    debug!(
                        "FeedPublisher: No previous version for feed {}. Will treat as new.",
                        feed.name
                    );
                    // Continue anyway - we'll create the first version if there's an update
                    continue;
                }
            };

            // Fetch current state from source
            let curr_check = match self.sources.get_latest_by_url(&feed.url).await {
                Ok(SourceResult::Series(series)) => series,
                Err(e) => {
                    if matches!(e, SourceError::FinishedSeries { .. }) {
                        info!(
                            "FeedPublisher: Feed {} is finished. Removing from database.",
                            feed.name
                        );
                        self.db.feed_table.delete(&feed.id).await?;
                    } else {
                        error!("FeedPublisher: Error fetching feed {}: {}", feed.name, e);
                    }
                    continue;
                }
            };

            debug!(
                "FeedPublisher: Current version for feed {}: {}",
                feed.name, curr_check.latest
            );

            // Check if version changed
            if curr_check.latest == prev_version.version {
                debug!("FeedPublisher: No new version for feed {}.", feed.name);
                continue;
            }

            info!(
                "FeedPublisher: New version found for feed {}: {} -> {}",
                feed.name, prev_version.version, curr_check.latest
            );

            // Insert new version into database
            let new_version = crate::database::model::FeedVersionModel {
                id: 0, // Will be set by database
                feed_id: feed.id,
                version: curr_check.latest.clone(),
                published: curr_check.published,
            };
            let version_id = self.db.feed_version_table.insert(&new_version).await?;

            // Publish update event
            info!(
                "FeedPublisher: Publishing update event for feed {}.",
                feed.name
            );
            let event = FeedUpdateEvent {
                feed_id: feed.id,
                version_id,
                title: curr_check.title,
                previous_version: prev_version.version,
                current_version: curr_check.latest,
                url: feed.url,
                published: curr_check.published,
            };
            self.event_bus.publish(event);
        }

        debug!("FeedPublisher: Finished checking for feed updates.");
        Ok(())
    }
}
