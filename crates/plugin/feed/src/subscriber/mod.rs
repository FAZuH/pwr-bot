//! Event subscribers that handle published events.

pub mod discord_dm;
pub mod discord_guild;

use anyhow::Result;

/// Handles events published to the subscriber.
#[async_trait::async_trait]
pub trait Subscriber<E> {
    /// Called when an event of type `E` is published.
    async fn callback(&self, event: E) -> Result<()>;
}
