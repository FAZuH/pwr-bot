use crate::event::manga_update_event::MangaUpdateEvent;
use async_trait::async_trait;
use std::any::Any;

#[async_trait]
pub trait Action: Send + Sync {
    async fn run(&self, event: &MangaUpdateEvent) -> anyhow::Result<()>;
    fn as_any(&self) -> &dyn Any;
}
