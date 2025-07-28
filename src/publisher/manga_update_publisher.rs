use crate::database::database::Database;
use crate::database::table::table::Table;
use crate::event::event_bus::EventBus;
use crate::event::manga_update_event::MangaUpdateEvent;
use crate::source::manga_dex_source::MangaDexSource;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub struct MangaUpdatePublisher {
    db: Arc<Database>,
    event_bus: Arc<EventBus>,
    source: Arc<MangaDexSource>,
    running: AtomicBool,
    interval: Duration,
}

impl MangaUpdatePublisher {
    pub fn new(db: Arc<Database>, event_bus: Arc<EventBus>, source: Arc<MangaDexSource>, poll_interval: Duration) -> Arc<Self> {
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
            self.spawn_check_loop();
        }
        Ok(())
    }

    pub fn stop(self: Arc<Self>) -> anyhow::Result<()> {
        self.running.store(false, Ordering::SeqCst);
        Ok(())
    }

    fn spawn_check_loop(self: Arc<Self>) {
        let mut interval = tokio::time::interval(self.interval);
        tokio::spawn(async move {
            loop {
                interval.tick().await;
                if !self.running.load(Ordering::SeqCst) {
                    break;
                }
                if let Err(e) = self.check_updates().await {
                    eprintln!("Error checking updates: {}", e);
                }
            }
        });
    }

    async fn check_updates(&self) -> anyhow::Result<()> {
        // Init step
        // 1. Get subscriptions from database
        let subscribers = self.db.subscribers_table.select_all_by_type("manga").await?;

        // 2. Get unique manga ids to fetch
        let mut latest_update_ids = HashSet::<u32>::new();
        for subs in subscribers {
            if !latest_update_ids.insert(subs.latest_update_id) {
                continue;
            };

            let mut prev_check = self.db
                .latest_updates_table
                .select(&subs.latest_update_id)
                .await?;

            // Check step
            // 3. Fetch latest manga chapters from sources using the unique manga ids
            let curr = self.source.get_latest(&prev_check.series_id).await?;
            // 4. Compare chapters
            if curr.chapter_id == prev_check.series_latest {
                continue;
            }

            // Handle update event
            // 5. Insert new updates into database
            prev_check.series_latest = curr.chapter_id.clone();
            prev_check.series_published = curr.published;
            self.db.latest_updates_table.update(&prev_check).await?;

            // 6. Publish events to event bus
            let event: MangaUpdateEvent = curr.into();
            self.event_bus.publish(event).await;
        }
        Ok(())
    }
}
