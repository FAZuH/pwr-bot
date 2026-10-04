//! Internal service for bot metadata and maintenance operations.

use std::sync::Arc;

use crate::entity::BotMetaEntity;
use crate::entity::BotMetaKey;
use crate::repo::error::DatabaseError;
use crate::repo::traits::*;
use crate::service::traits::InternalOps;

#[async_trait::async_trait]
impl InternalOps for InternalService {
    async fn get_meta(&self, key: BotMetaKey) -> Result<Option<String>, DatabaseError> {
        self.get_meta(key).await
    }

    async fn set_meta(&self, key: BotMetaKey, value: String) -> Result<(), DatabaseError> {
        self.set_meta(key, value).await
    }
}

/// Internal service for metadata and maintenance operations.
pub struct InternalService {
    bot_meta: Arc<dyn BotMetaRepository + Send + Sync>,
}

impl InternalService {
    /// Creates a new internal service.
    pub fn new(bot_meta: Arc<dyn BotMetaRepository + Send + Sync>) -> Self {
        Self { bot_meta }
    }

    /// Get a metadata value by key.
    pub async fn get_meta(&self, key: BotMetaKey) -> Result<Option<String>, DatabaseError> {
        let result: Option<BotMetaEntity> = self.bot_meta.select(&key.into()).await?;
        Ok(result.map(|m| m.value))
    }

    /// Set a metadata value by key (upsert).
    pub async fn set_meta(
        &self,
        key: BotMetaKey,
        value: impl Into<String>,
    ) -> Result<(), DatabaseError> {
        let model = BotMetaEntity {
            key: key.into(),
            value: value.into(),
        };
        self.bot_meta.replace(&model).await?;
        Ok(())
    }
}
