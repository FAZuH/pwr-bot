//! Business logic services for settings, internal operations, and voice tracking.

use std::sync::Arc;

use crate::repo::traits::Repos;
use crate::service::internal::InternalService;
use crate::service::settings::SettingsService;
use crate::service::traits::*;
use crate::service::voice_tracking::VoiceTrackingService;

pub mod error;
pub mod internal;
pub mod settings;
pub mod traits;
pub mod voice_tracking;

/// Container for all application services.
pub struct Services {
    pub settings: Arc<dyn SettingsProvider>,
    pub voice_tracking: Arc<dyn VoiceTracker>,
    pub internal: Arc<dyn InternalOps>,
}

impl Services {
    /// Creates and initializes all services.
    ///
    /// Each service extracts its repo handles from the factory at construction
    /// time, not per-operation. See [`Repos`] for the factory trait.
    pub async fn new(repos: Arc<dyn Repos + Send + Sync>) -> anyhow::Result<Self> {
        let settings = Arc::new(SettingsService::new(Arc::from(repos.server_settings())));
        let voice_tracking = Arc::new(
            VoiceTrackingService::new(
                Arc::from(repos.voice_sessions()),
                Arc::from(repos.server_settings()),
            )
            .await?,
        );
        let internal = Arc::new(InternalService::new(
            Arc::from(repos.feed_dump()),
            Arc::from(repos.bot_meta()),
        ));
        Ok(Self {
            settings,
            voice_tracking,
            internal,
        })
    }
}
