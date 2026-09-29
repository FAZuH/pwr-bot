//! Business logic services for settings and internal operations.

use std::sync::Arc;

use crate::repo::traits::Repos;
use crate::service::internal::InternalService;
use crate::service::traits::*;

pub mod error;
pub mod internal;
pub mod traits;

/// Container for the services the host still owns.
pub struct Services {
    pub internal: Arc<dyn InternalOps>,
}

impl Services {
    /// Creates the host services from the repository factory.
    pub async fn new(repos: Arc<dyn Repos + Send + Sync>) -> anyhow::Result<Self> {
        let internal = Arc::new(InternalService::new(
            Arc::from(repos.feed_dump()),
            Arc::from(repos.bot_meta()),
        ));
        Ok(Self { internal })
    }
}
