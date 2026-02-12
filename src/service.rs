//! Business logic services for feed subscriptions and voice tracking.

use std::sync::Arc;

use crate::database::Database;
use crate::feed::platforms::Platforms;
use crate::service::feed_subscription_service::FeedSubscriptionService;
use crate::service::voice_tracking_service::VoiceTrackingService;

pub mod error;
pub mod feed_subscription_service;
pub mod voice_tracking_service;

/// Container for all application services.
pub struct Services {
    pub feed_subscription: Arc<FeedSubscriptionService>,
    pub voice_tracking: Arc<VoiceTrackingService>,
}

impl Services {
    /// Creates and initializes all services.
    pub async fn new(db: Arc<Database>, platforms: Arc<Platforms>) -> anyhow::Result<Self> {
        let voice_tracking = Arc::new(VoiceTrackingService::new(db.clone()).await?);

        Ok(Self {
            feed_subscription: Arc::new(FeedSubscriptionService::new(
                db.clone(),
                platforms.clone(),
            )),
            voice_tracking,
        })
    }
}
