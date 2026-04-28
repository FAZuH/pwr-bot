//! Event subscribers that handle published events.

pub mod discord_dm_subscriber;
pub mod discord_guild_subscriber;
pub mod voice_state_subscriber;

use anyhow::Result;

/// Trait for event subscribers.
#[async_trait::async_trait]
pub trait Subscriber<E> {
    /// Called when an event of type E is published.
    async fn callback(&self, event: E) -> Result<()>;
}
