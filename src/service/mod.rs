use std::path::PathBuf;
use std::sync::Arc;

use crate::database::Database;
use crate::feed::platforms::Platforms;
use crate::service::feed_subscription_service::FeedSubscriptionService;
use crate::service::voice_heartbeat::VoiceHeartbeatManager;
use crate::service::voice_tracking_service::VoiceTrackingService;

pub mod error;
pub mod feed_subscription_service;
pub mod voice_heartbeat;
pub mod voice_tracking_service;

pub struct Services {
    pub feed_subscription: Arc<FeedSubscriptionService>,
    pub voice_tracking: Arc<VoiceTrackingService>,
    pub voice_heartbeat: Option<Arc<VoiceHeartbeatManager>>,
}

impl Services {
    pub async fn new(
        db: Arc<Database>,
        platforms: Arc<Platforms>,
        data_dir: impl Into<PathBuf>,
    ) -> anyhow::Result<Self> {
        let voice_tracking = Arc::new(VoiceTrackingService::new(db.clone()).await?);
        let voice_heartbeat =
            Arc::new(VoiceHeartbeatManager::new(data_dir, voice_tracking.clone()));

        Ok(Self {
            feed_subscription: Arc::new(FeedSubscriptionService::new(
                db.clone(),
                platforms.clone(),
            )),
            voice_tracking,
            voice_heartbeat: Some(voice_heartbeat),
        })
    }
}
