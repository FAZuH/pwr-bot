use std::collections::HashSet;
use std::time::Duration;
use crate::config::Config;
use crate::database::database::Database;
use crate::database::table::table::Table;
use crate::event::anime_update_event::AnimeUpdateEvent;
use crate::source::manga_dex_source::MangaDexSource;
use crate::event::event_bus::EventBus;

pub struct MangaUpdatePublisher {
    db: &'static Database,
    event_bus: &'static EventBus,
    source: MangaDexSource,
    running: bool,
    interval: Duration
}

impl MangaUpdatePublisher {
    pub async fn new(config: &Config, db: &'static Database, event_bus: &'static EventBus) -> anyhow::Result<Self> {
        Ok(Self {
            db: db,
            event_bus: event_bus,
            source: MangaDexSource::new(),
            running: false,
            interval: Duration::new(config.poll_interval, 0)
        })
    }

    pub fn start(&'static mut self) -> anyhow::Result<()> {
        if !self.running {
            self.running = true;
            self.spawn_check_loop();
        }
        Ok(())
    }

    pub fn stop(&mut self) -> anyhow::Result<()> {
        self.running = false;
        Ok(())
    }

    fn spawn_check_loop(&'static self) {
        let interval_duration = self.interval;  // Duration implements Copy, no need to clone

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(interval_duration);
            loop {
                interval.tick().await;
                // Pass references instead of cloning every iteration
                if let Err(e) = Self::check_updates(&self).await {
                    eprintln!("Error checking updates: {}", e);
                }
            }
        });
    }

    async fn check_updates(
        &self
    ) -> anyhow::Result<()> {
        // Init step
        // 1. Get subscriptions from databaes
        let db = self.db;
        let source = &self.source;
        let subscribers = db.subscribers_table.select_all_by_type("manga").await?;

        // 2. Get unique manga ids to fetch
        let mut latest_update_ids = HashSet::<u32>::new();
        for subs in subscribers {
            if latest_update_ids.insert(subs.latest_updates_id) {
                let mut prev_check = db.latest_updates_table.select(&subs.latest_updates_id).await?;
                // Check step
                // 3. Fetch latest manga chapters from sources using the unique manga ids
                if let Some(curr) = source.get_latest(&prev_check.series_id).await? {
                    // 4. Compare chapters
                    if curr.chapter_id == prev_check.series_latest { continue; }

                    // Handle update event
                    // 5. Insert new updates into database
                    prev_check.series_latest = curr.series_id.clone();
                    prev_check.series_published = curr.published;
                    db.latest_updates_table.update(&prev_check).await?;

                    // 6. Publish events to event bus
                    let event: AnimeUpdateEvent = curr.into();
                    self.event_bus.publish(&event);
                }
            }
        }
        Ok(())
    }
}
