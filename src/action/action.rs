use async_trait::async_trait;
use crate::event::new_chapter_event::NewChapterEvent;
use std::any::Any;

#[async_trait]
pub trait Action: Send + Sync {
    async fn run(&self, event: &NewChapterEvent) -> anyhow::Result<()>;
    fn as_any(&self) -> &dyn Any;
}
