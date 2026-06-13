//! Business logic services for feed subscriptions and voice tracking.

use std::sync::Arc;

use crate::repo::traits::Repos;
use crate::service::internal::InternalService;
use crate::service::settings::SettingsService;
use crate::service::traits::*;

pub mod error;
pub mod feed_subscription;
pub mod internal;
pub mod settings;
pub mod traits;

/// Container for all application services.
pub struct Services {
    pub settings: Arc<dyn SettingsProvider>,
    pub feed_subscription: Arc<dyn FeedSubscriptionProvider>,
    pub internal: Arc<dyn InternalOps>,
}

impl Services {
    /// Creates and initializes all services.
    ///
    /// Each service extracts its repo handles from the factory at construction
    /// time, not per-operation. See [`Repos`] for the factory trait.
    pub async fn new(
        repos: Arc<dyn Repos + Send + Sync>,
    ) -> anyhow::Result<Self> {
        let settings_svc = Arc::new(SettingsService::new(Arc::from(repos.server_settings())));
        let settings = settings_svc.clone() as Arc<dyn SettingsProvider>;
        let feed_subscription = settings_svc as Arc<dyn FeedSubscriptionProvider>;
        let internal = Arc::new(InternalService::new(
            Arc::from(repos.feed()),
            Arc::from(repos.feed_item()),
            Arc::from(repos.subscriber()),
            Arc::from(repos.feed_subscription()),
            Arc::from(repos.bot_meta()),
        ));

        Ok(Self {
            settings,
            feed_subscription,
            internal,
        })
    }
}
