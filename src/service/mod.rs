//! Business logic services for feed subscriptions and voice tracking.

use std::sync::Arc;

use crate::feed::Platforms;
use crate::repo::traits::Repos;
use crate::service::feed_subscription::FeedSubscriptionService;
use crate::service::internal::InternalService;
use crate::service::settings::SettingsService;
use crate::service::traits::*;
use crate::service::voice_tracking::VoiceTrackingService;

pub mod error;
pub mod feed_subscription;
pub mod internal;
pub mod settings;
pub mod traits;
pub mod voice_tracking;

/// Container for all application services.
pub struct Services {
    pub settings: Arc<dyn SettingsProvider>,
    pub feed_subscription: Arc<dyn FeedSubscriptionProvider>,
    pub voice_tracking: Arc<dyn VoiceTracker>,
    pub internal: Arc<dyn InternalOps>,
}

impl Services {
    /// Creates and initializes all services.
    ///
    /// Each service extracts its repo handles from the factory at construction
    /// time, not per-operation. See [`Repos`] for the factory trait.
    pub async fn new(
        repos: Arc<dyn Repos + Send + Sync>,
        platforms: Arc<Platforms>,
    ) -> anyhow::Result<Self> {
        let settings = Arc::new(SettingsService::new(Arc::from(repos.server_settings())));
        let voice_tracking = Arc::new(
            VoiceTrackingService::new(
                Arc::from(repos.voice_sessions()),
                Arc::from(repos.server_settings()),
            )
            .await?,
        );
        let internal = Arc::new(InternalService::new(
            Arc::from(repos.feed()),
            Arc::from(repos.feed_item()),
            Arc::from(repos.subscriber()),
            Arc::from(repos.feed_subscription()),
            Arc::from(repos.bot_meta()),
        ));
        let feed_subscription = Arc::new(FeedSubscriptionService::new(
            Arc::from(repos.feed()),
            Arc::from(repos.feed_item()),
            Arc::from(repos.subscriber()),
            Arc::from(repos.feed_subscription()),
            Arc::from(repos.server_settings()),
            platforms.clone(),
        ));

        Ok(Self {
            settings,
            feed_subscription,
            voice_tracking,
            internal,
        })
    }
}
