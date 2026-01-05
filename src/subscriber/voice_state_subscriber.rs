use std::sync::Arc;

use anyhow::Result;
use chrono::Utc;

use crate::database::model::VoiceSessionsModel;
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
        let new = event.new;

        let join_time = match new.request_to_speak_timestamp {
            Some(res) => res.to_utc(),
            None => return Ok(()),
        };
        let user_id = new.user_id.get();
        let guild_id = match new.guild_id {
            Some(res) => res.get(),
            None => return Ok(()),
        };
        let channel_id = match new.channel_id {
            Some(res) => res.get(),
            None => return Ok(()),
        };
        let leave_time = Utc::now();

        let model = VoiceSessionsModel {
            user_id,
            guild_id,
            channel_id,
            join_time,
            leave_time,
            ..Default::default()
        };

        self.services.voice_tracking.insert(&model).await?;

        Ok(())
    }
}

#[async_trait::async_trait]
impl Subscriber<VoiceStateEvent> for VoiceStateSubscriber {
    async fn callback(&self, event: VoiceStateEvent) -> Result<()> {
        self.callback(event).await
    }
}
