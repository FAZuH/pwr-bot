//! Business logic interfaces for host services.

use async_trait::async_trait;

use crate::entity::*;
use crate::repo::error::DatabaseError;

/// Internal bot operations and metadata management.
#[async_trait]
pub trait InternalOps: Send + Sync {
    /// Retrieves a piece of metadata by key.
    async fn get_meta(&self, key: BotMetaKey) -> Result<Option<String>, DatabaseError>;

    /// Stores a piece of metadata.
    async fn set_meta(&self, key: BotMetaKey, value: String) -> Result<(), DatabaseError>;
}
