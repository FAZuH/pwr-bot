use crate::database::database::Database;
use crate::database::table::table::Table;
use crate::event::event_bus::EventBus;
use crate::event::manga_update_event::MangaUpdateEvent;
use crate::source::manga_dex_source::MangaDexSource;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use log::{info, error, debug};

pub struct MangaUpdatePublisher {
    db: Arc<Database>,
    event_bus: Arc<EventBus>,
    source: Arc<MangaDexSource>,
    running: AtomicBool,
    interval: Duration,
}

impl MangaUpdatePublisher {
    pub fn new(db: Arc<Database>, event_bus: Arc<EventBus>, source: Arc<MangaDexSource>, poll_interval: Duration) -> Arc<Self> {
        info!("Initializing MangaUpdatePublisher with poll interval {:?}", poll_interval);
        Arc::new(Self {
            db,
            event_bus,
            source,
            running: AtomicBool::new(false),
            interval: poll_interval,
        })
    }

    pub fn start(self: Arc<Self>) -> anyhow::Result<()> {
        if !self.running.load(Ordering::SeqCst) {
            self.running.store(true, Ordering::SeqCst);
            info!("Starting MangaUpdatePublisher check loop.");
            self.spawn_check_loop();
        }
        Ok(())
    }

    pub fn stop(self: Arc<Self>) -> anyhow::Result<()> {
        info!("Stopping MangaUpdatePublisher check loop.");
        self.running.store(false, Ordering::SeqCst);
        Ok(())
    }

    fn spawn_check_loop(self: Arc<Self>) {
        let mut interval = tokio::time::interval(self.interval);
        tokio::spawn(async move {
            loop {
                interval.tick().await;
                if !self.running.load(Ordering::SeqCst) {
                    info!("MangaUpdatePublisher: Stopping check loop.");
                    break;
                }
                if let Err(e) = self.check_updates().await {
                    error!("MangaUpdatePublisher: Error checking updates: {}", e);
                }
            }
        });
    }

    async fn check_updates(&self) -> anyhow::Result<()> {
        // Init step
        // 1. Get subscriptions from database
        debug!("MangaUpdatePublisher: Checking for manga updates.");
        let subscribers = self.db.subscribers_table.select_all_by_type("manga").await?;
        info!("MangaUpdatePublisher: Found {} manga subscriptions.", subscribers.len());

        // 2. Get unique manga ids to fetch
        let mut latest_update_ids = HashSet::<u32>::new();
        for subs in subscribers {
            if !latest_update_ids.insert(subs.latest_update_id) {
                continue;
            };
            debug!("MangaUpdatePublisher: Checking for updates for subscription ID: {}", subs.latest_update_id);

            let mut prev_check = self.db
                .latest_updates_table
                .select(&subs.latest_update_id)
                .await?;
            debug!("MangaUpdatePublisher: Previous check for series ID {}: chapter {}", prev_check.series_id, prev_check.series_latest);

            // Check step
            // 3. Fetch latest manga chapters from sources using the unique manga ids
            let curr = self.source.get_latest(&prev_check.series_id).await?;
            debug!("MangaUpdatePublisher: Current latest for series ID {}: chapter {}", prev_check.series_id, curr.chapter_id);
            // 4. Compare chapters
            if curr.chapter_id == prev_check.series_latest {
                debug!("MangaUpdatePublisher: No new chapter for series ID {}.", prev_check.series_id);
                continue;
            }

            // Handle update event
            info!("MangaUpdatePublisher: New chapter found for series ID {}: {} -> {}. Updating database.", prev_check.series_id, prev_check.series_latest, curr.chapter_id);
            // 5. Insert new updates into database
            prev_check.series_latest = curr.chapter_id.clone();
            prev_check.series_published = curr.published;
            self.db.latest_updates_table.update(&prev_check).await?;

            // 6. Publish events to event bus
            let event: MangaUpdateEvent = curr.into();
            self.event_bus.publish(event).await;
        }
        debug!("MangaUpdatePublisher: Finished checking for manga updates.");
        Ok(())
    }
}
