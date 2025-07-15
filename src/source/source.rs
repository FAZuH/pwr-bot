use async_trait::async_trait;
use crate::event::new_chapter_event::NewChapterEvent;

#[async_trait]
pub trait UpdateSource: Send + Sync {
    async fn check_update(&self, series_id: &str) -> anyhow::Result<Option<NewChapterEvent>>;
}
