use std::sync::Arc;

use anyhow::Result;

use crate::event::VoiceStateEvent;
use crate::service::Services;
use crate::subscriber::Subscriber;

pub struct VoiceStateSubscriber {
    services: Arc<Services>,
}

impl VoiceStateSubscriber {
    pub fn new(services: Arc<Services>) -> Self {
        Self { services }
    }
    pub async fn callback(&self, event: VoiceStateEvent) -> Result<()> {
        todo!()
    }
}

#[async_trait::async_trait]
impl Subscriber<VoiceStateEvent> for VoiceStateSubscriber {
    async fn callback(&self, event: VoiceStateEvent) -> Result<()> {
        self.callback(event).await
    }
}
