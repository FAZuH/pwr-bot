pub mod discord_dm_subscriber;
pub mod discord_guild_subscriber;
pub mod voice_state_subscriber;

use anyhow::Result;

#[async_trait::async_trait]
pub trait Subscriber<E> {
    async fn callback(&self, event: E) -> Result<()>;
}
