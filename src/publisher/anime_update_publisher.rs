use crate::database::database::Database;
use crate::database::table::table::Table;
use crate::event::anime_update_event::AnimeUpdateEvent;
use crate::event::event_bus::EventBus;
use crate::source::ani_list_source::AniListSource;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use log::{info, error, debug};

pub struct AnimeUpdatePublisher {
    db: Arc<Database>,
    event_bus: Arc<EventBus>,
    source: Arc<AniListSource>,
    poll_interval: Duration,
    running: AtomicBool,
}

impl AnimeUpdatePublisher {
    pub fn new(db: Arc<Database>, event_bus: Arc<EventBus>, source: Arc<AniListSource>, poll_interval: Duration) -> Arc<Self> {
        info!("Initializing AnimeUpdatePublisher with poll interval {:?}", poll_interval);
        Arc::new(Self {
            db,
            event_bus,
            source,
            poll_interval,
            running: AtomicBool::new(false),
        })
    }

    pub fn start(self: Arc<Self>) -> anyhow::Result<()> {
        if !self.running.load(Ordering::SeqCst) {
            self.running.store(true, Ordering::SeqCst);
            info!("Starting AnimeUpdatePublisher check loop.");
            self.spawn_check_loop();
        }
        Ok(())
    }

    pub fn stop(self: Arc<Self>) -> anyhow::Result<()> {
        info!("Stopping AnimeUpdatePublisher check loop.");
        self.running.store(false, Ordering::SeqCst);
        Ok(())
    }

    fn spawn_check_loop(self: Arc<Self>) {
        let mut interval = tokio::time::interval(self.poll_interval);
        tokio::spawn(async move {
            loop {
                interval.tick().await;
                debug!("AnimeUpdatePublisher: Tick.");
                if !self.running.load(Ordering::SeqCst) {
                    info!("AnimeUpdatePublisher: Stopping check loop.");
                    break;
                }
                if let Err(e) = self.check_updates().await {
                    error!("AnimeUpdatePublisher: Error checking updates: {}", e);
                }
            }
        });
    }

    async fn check_updates(&self) -> anyhow::Result<()> {
        // 1. Get subscriptions from database
        let db = &self.db;
        let source = &self.source;
        debug!("AnimeUpdatePublisher: Checking for anime updates.");
        let latest_updates = db.latest_updates_table.select_all_by_type("anime").await?;
        info!("AnimeUpdatePublisher: Found {} anime subscriptions.", latest_updates.len());

        for mut prev_check in latest_updates {
            // 2. No subscribers to prev_check.id => Don't publish
            if db.subscribers_table.select_all_by_latest_update(prev_check.id).await?.is_empty() {
                continue;
            }

            // 3. Fetch latest anime episodes from sources
            let curr = source.get_latest(&prev_check.series_id).await?;
            debug!("AnimeUpdatePublisher: Current latest for series ID {}: episode {}", prev_check.series_id, curr.episode);

            // 4. Compare chapters
            if curr.episode == prev_check.series_latest {
                debug!("AnimeUpdatePublisher: No new episode for series ID {}.", prev_check.series_id);
                continue;
            }
            info!("AnimeUpdatePublisher: New episode found for series ID {}: {} -> {}. Updating database.", prev_check.series_id, prev_check.series_latest, curr.episode);

            // Handle update event
            // 5. Insert new updates into database
            prev_check.series_latest = curr.episode.clone();
            prev_check.series_published = curr.published;
            db.latest_updates_table.update(&prev_check).await?;

            // 6. Publish events to event bus
            info!("AnimeUpdatePublisher: Publishing update event for series ID {}.", prev_check.series_id);
            let event: AnimeUpdateEvent = curr.into();
            self.event_bus.publish(event).await;
        }

        debug!("AnimeUpdatePublisher: Finished checking for anime updates.");
        Ok(())
    }
}
