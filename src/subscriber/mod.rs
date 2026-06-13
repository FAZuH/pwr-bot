//! Event subscribers that handle published events.
//!
//! Core subscribers: Discord message delivery for feed updates.
//! Voice subscribers have been moved to the voice plugin.

use anyhow::Result;

/// Trait for event subscribers.
#[async_trait::async_trait]
pub trait Subscriber<E> {
    /// Called when an event of type E is published.
    async fn callback(&self, event: E) -> Result<()>;
}
