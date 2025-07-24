use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use crate::config::Config;
use crate::database::database::Database;
use crate::database::table::table::Table;
use crate::event::anime_update_event::AnimeUpdateEvent;
use crate::source::ani_list_source::AniListSource;
use crate::event::event_bus::EventBus;

pub struct AnimeUpdatePublisher {
    db: Arc<Database>,
    event_bus: Arc<EventBus>,
    source: AniListSource,
    running: bool,
    interval: Duration
}

impl AnimeUpdatePublisher {
    pub async fn new(config: &Config, db: Arc<Database>, event_bus: Arc<EventBus>) -> anyhow::Result<Self> {
        Ok(Self {
            db,
            event_bus,
            source: AniListSource::new(),
            running: false,
            interval: Duration::new(config.poll_interval, 0)
        })
    }

    pub fn start(&mut self) -> anyhow::Result<()> {
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

    fn spawn_check_loop(&self) {
        let interval_duration = self.interval;
        let self_clone = self.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(interval_duration);
            loop {
                interval.tick().await;
                if let Err(e) = self_clone.check_updates().await {
                    eprintln!("Error checking updates: {}", e);
                }
            }
        });
    }

    async fn check_updates(
        &self
    ) -> anyhow::Result<()> {
        // Init step
        // 1. Get subscriptions from database
        let db = &self.db;
        let source = &self.source;
        let subscribers = db.subscribers_table.select_all_by_type("anime").await?;

        // 2. Get unique anime ids to fetch
        let mut latest_update_ids = HashSet::<u32>::new();
        for subs in subscribers {
            if latest_update_ids.insert(subs.latest_updates_id) {
                let mut prev_check = db.latest_updates_table.select(&subs.latest_updates_id).await?;
                // Check step
                // 3. Fetch latest anime chapters from sources using the unique anime ids
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

impl Clone for AnimeUpdatePublisher {
    fn clone(&self) -> Self {
        Self {
            db: self.db.clone(),
            event_bus: self.event_bus.clone(),
            source: self.source.clone(),
            running: self.running,
            interval: self.interval,
        }
    }
}
