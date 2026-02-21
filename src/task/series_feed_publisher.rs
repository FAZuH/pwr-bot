//! Background task for polling feed updates.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;

use log::debug;
use log::error;
use log::info;
use tokio::time::Sleep;
use tokio::time::sleep;

use crate::event::FeedUpdateData;
use crate::event::FeedUpdateEvent;
use crate::event::event_bus::EventBus;
use crate::entity::FeedModel;
use crate::service::feed_subscription_service::FeedSubscriptionService;
use crate::service::feed_subscription_service::FeedUpdateResult;

/// Task that periodically checks feeds for updates.
pub struct SeriesFeedPublisher {
    service: Arc<FeedSubscriptionService>,
    event_bus: Arc<EventBus>,
    poll_interval: Duration,
    running: AtomicBool,
}

impl SeriesFeedPublisher {
    /// Creates a new feed publisher with the given configuration.
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

    /// Starts the feed polling loop.
    pub fn start(self: Arc<Self>) -> anyhow::Result<()> {
        if !self.running.load(Ordering::SeqCst) {
            self.running.store(true, Ordering::SeqCst);
            info!("Starting FeedPublisher check loop.");
            self.spawn_check_loop();
        }
        Ok(())
    }

    /// Stops the feed polling loop.
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

                let feed = Arc::new(feed);
                let feed_info = Arc::new(feed_info);
                let old_feed_item = old_item.map(Arc::new);
                let new_feed_item = Arc::new(new_item);

                let data = FeedUpdateData {
                    feed: feed.clone(),
                    feed_info: feed_info.clone(),
                    old_feed_item: old_feed_item.clone(),
                    new_feed_item: new_feed_item.clone(),
                };

                // Publish update event
                info!("Publishing update event for {}.", self.get_feed_desc(&feed));
                let event = FeedUpdateEvent::new(data);
                self.event_bus.publish(event);
                Ok(())
            }
        }
    }

    fn get_feed_desc(&self, feed: &FeedModel) -> String {
        format!("feed id `{}` ({})", feed.id, feed.name)
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
