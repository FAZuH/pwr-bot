//! Business logic services for feed subscriptions and voice tracking.

use std::sync::Arc;

use crate::feed::Platforms;
use crate::repo::Repository;
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
    pub async fn new(db: Arc<Repository>, platforms: Arc<Platforms>) -> anyhow::Result<Self> {
        let settings = Arc::new(SettingsService::new(db.clone()));
        let voice_tracking = Arc::new(VoiceTrackingService::new(db.clone()).await?);
        let internal = Arc::new(InternalService::new(db.clone()));
        let feed_subscription =
            Arc::new(FeedSubscriptionService::new(db.clone(), platforms.clone()));

        Ok(Self {
            settings,
            feed_subscription,
            voice_tracking,
            internal,
        })
    }
}
