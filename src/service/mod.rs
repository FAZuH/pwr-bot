use std::sync::Arc;

use crate::repo::traits::Repos;
use crate::service::internal::InternalService;
use crate::service::settings::SettingsService;
use crate::service::traits::*;

pub mod error;
pub mod internal;
pub mod settings;
pub mod traits;

pub struct Services {
    pub settings: Arc<dyn SettingsProvider>,
    pub internal: Arc<dyn InternalOps>,
}

impl Services {
    pub async fn new(repos: Arc<dyn Repos + Send + Sync>) -> anyhow::Result<Self> {
        let settings = Arc::new(SettingsService::new(Arc::from(repos.server_settings())));
        let internal = Arc::new(InternalService::new(Arc::from(repos.bot_meta())));
        Ok(Self { settings, internal })
    }
}
