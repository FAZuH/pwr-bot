//! Business logic services for feed subscriptions and voice tracking.

use std::sync::Arc;

use crate::feed::platforms::Platforms;
use crate::repository::Repository;
use crate::service::feed_subscription_service::FeedSubscriptionService;
use crate::service::internal_service::InternalService;
use crate::service::settings_service::SettingsService;
use crate::service::traits::*;
use crate::service::voice_tracking_service::VoiceTrackingService;

pub mod error;
pub mod feed_subscription_service;
pub mod internal_service;
pub mod settings_service;
pub mod traits;
pub mod voice_tracking_service;

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
