use crate::database::database::Database;
use crate::database::table::Table;
use crate::event::event_bus::EventBus;
use crate::event::series_update_event::SeriesUpdateEvent;
use crate::source::model::SourceResult;
use crate::source::sources::Sources;
use log::{debug, error, info};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub struct SeriesPublisher {
    db: Arc<Database>,
    event_bus: Arc<EventBus>,
    sources: Arc<Sources>,
    poll_interval: Duration,
    running: AtomicBool,
}

impl SeriesPublisher {
    pub fn new(
        db: Arc<Database>,
        event_bus: Arc<EventBus>,
        source: Arc<Sources>,
        poll_interval: Duration,
    ) -> Arc<Self> {
        info!(
            "Initializing SeriesPublisher with poll interval {:?}",
            poll_interval
        );
        Arc::new(Self {
            db,
            event_bus,
            sources: source,
            poll_interval,
            running: AtomicBool::new(false),
        })
    }

    pub fn start(self: Arc<Self>) -> anyhow::Result<()> {
        if !self.running.load(Ordering::SeqCst) {
            self.running.store(true, Ordering::SeqCst);
            info!("Starting SeriesPublisher check loop.");
            self.spawn_check_loop();
        }
        Ok(())
    }

    pub fn stop(self: Arc<Self>) -> anyhow::Result<()> {
        info!("Stopping SeriesPublisher check loop.");
        self.running.store(false, Ordering::SeqCst);
        Ok(())
    }

    fn spawn_check_loop(self: Arc<Self>) {
        let mut interval = tokio::time::interval(self.poll_interval);
        tokio::spawn(async move {
            loop {
                interval.tick().await;
                debug!("SeriesPublisher: Tick.");
                if !self.clone().running.load(Ordering::SeqCst) {
                    info!("SeriesPublisher: Stopping check loop.");
                    break;
                }
                if let Err(e) = self.clone().check_updates().await {
                    error!("SeriesPublisher: Error checking updates: {}", e);
                }
            }
        });
    }

    async fn check_updates(self: Arc<Self>) -> anyhow::Result<()> {
        // 1. Get subscriptions from database
        let db = &self.db;
        let sources = &self.sources;
        debug!("SeriesPublisher: Checking for series updates.");
        let latest_updates = db.latest_results_table.select_all_by_tag("series").await?;
        info!(
            "SeriesPublisher: Found {} series subscriptions.",
            latest_updates.len()
        );

        for mut prev_check in latest_updates {
            // 2. No subscribers to prev_check.id => Don't publish
            if db
                .subscribers_table
                .select_all_by_latest_results(prev_check.id)
                .await?
                .is_empty()
            {
                continue;
            }

            // 3. Fetch latest series latests from sources
            let curr_check = match sources.get_latest_by_url(&prev_check.url).await {
                Ok(SourceResult::Series(series)) => series,
                _ => continue,
            };
            // let curr = source.anilist_source.get_latest(&prev_check.series_url).await?;
            debug!(
                "SeriesPublisher: Current latest for series {}: latest: {}",
                prev_check.url, curr_check.latest
            );

            // 4. Compare chapters
            if curr_check.latest == prev_check.latest {
                debug!(
                    "SeriesPublisher: No new latest for series {}.",
                    prev_check.url
                );
                continue;
            }
            info!(
                "SeriesPublisher: New latest found for series {}: {} -> {}. Updating database.",
                prev_check.url, prev_check.latest, curr_check.latest
            );

            // Handle update event
            // 5. Update db with new updates.
            // TODO: This should be a subscriber
            let prev_latest_clone = prev_check.latest.clone();
            prev_check.latest = curr_check.latest.clone();
            prev_check.published = curr_check.published;
            db.latest_results_table.update(&prev_check).await?;

            // 6. Publish events to event bus
            info!(
                "SeriesPublisher: Publishing update event for series {}.",
                prev_check.url
            );
            let event = SeriesUpdateEvent {
                latest_results_id: prev_check.id,
                title: curr_check.title,
                previous: prev_latest_clone,
                current: curr_check.latest,
                url: prev_check.url,
                published: curr_check.published,
            };
            self.event_bus.publish(event).await;
        }

        debug!("SeriesPublisher: Finished checking for series updates.");
        Ok(())
    }
}
