use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use chrono::DateTime;
use chrono::Utc;
use log::debug;
use tokio::sync::Mutex;

use crate::database::model::VoiceSessionsModel;
use crate::event::VoiceStateEvent;
use crate::service::Services;
use crate::subscriber::Subscriber;

pub struct VoiceStateSubscriber {
    services: Arc<Services>,
    session_joins: Mutex<HashMap<String, DateTime<Utc>>>,
}

impl VoiceStateSubscriber {
    pub fn new(services: Arc<Services>) -> Self {
        Self {
            services,
            session_joins: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait::async_trait]
impl Subscriber<VoiceStateEvent> for VoiceStateSubscriber {
    /// From https://discord.com/developers/docs/events/gateway-events#voice-state-update:
    /// > Called when someone joins/leaves/moves voice channels. Inner payload is a voice state object.
    ///
    /// - event.old is None if and only if user joined a voice channel
    /// - event.new.channel_id is None if and only if user left a voice channel
    /// - event.old is Some and event.new.channel_id is Some if and only if user moved between
    /// voice channels
    async fn callback(&self, event: VoiceStateEvent) -> Result<()> {
        let old_channel = event.old.as_ref().and_then(|v| v.channel_id);
        let new_channel = event.new.channel_id;

        match (old_channel, new_channel) {
            // User joined
            (None, Some(channel_id)) => {
                debug!("User {} detected joining voice channel id {}", event.new.user_id.get(), channel_id.get());
                let join_time = Utc::now();
                self.session_joins
                    .lock()
                    .await
                    .insert(event.new.session_id.to_string(), join_time);

                let model = VoiceSessionsModel {
                    user_id: event.new.user_id.get(),
                    guild_id: event
                        .new
                        .guild_id
                        .ok_or(anyhow::anyhow!("Missing guild_id"))?
                        .get(),
                    channel_id: channel_id.get(),
                    join_time,
                    leave_time: join_time, // Or None if your DB supports it
                    ..Default::default()
                };

                self.services.voice_tracking.insert(&model).await?;
            }

            // User left
            (Some(old_channel_id), None) => {
                debug!("User {} detected leaving voice channel id {}", event.new.user_id.get(), old_channel_id.get());
                let old_state = event.old.as_ref().unwrap();
                let leave_time = Utc::now();

                let join_time = self
                    .session_joins
                    .lock()
                    .await
                    .remove(&old_state.session_id.to_string());

                if let Some(join_time) = join_time {
                    let model = VoiceSessionsModel {
                        user_id: old_state.user_id.get(),
                        guild_id: old_state
                            .guild_id
                            .ok_or(anyhow::anyhow!("Missing guild_id"))?
                            .get(),
                        channel_id: old_channel_id.get(),
                        join_time,
                        leave_time,
                        ..Default::default()
                    };
                    self.services.voice_tracking.replace(&model).await?;
                }
            }

            // User moved channels
            (Some(old_channel_id), Some(new_channel_id)) if old_channel_id != new_channel_id => {
                debug!("User {} detected moving from voice channel id {} to {}", event.new.user_id.get(), old_channel_id.get(), new_channel_id.get());
                let old_state = event.old.as_ref().unwrap();
                let now = Utc::now();

                // Close old session
                if let Some(join_time) = self
                    .session_joins
                    .lock()
                    .await
                    .remove(&old_state.session_id.to_string())
                {
                    let model = VoiceSessionsModel {
                        user_id: old_state.user_id.get(),
                        guild_id: old_state
                            .guild_id
                            .ok_or(anyhow::anyhow!("Missing guild_id"))?
                            .get(),
                        channel_id: old_channel_id.get(),
                        join_time,
                        leave_time: now,
                        ..Default::default()
                    };
                    self.services.voice_tracking.replace(&model).await?;
                }

                // Start new session
                self.session_joins
                    .lock()
                    .await
                    .insert(event.new.session_id.clone().to_string(), now);

                let model = VoiceSessionsModel {
                    user_id: event.new.user_id.get(),
                    guild_id: event
                        .new
                        .guild_id
                        .ok_or(anyhow::anyhow!("Missing guild_id"))?
                        .get(),
                    channel_id: new_channel_id.get(),
                    join_time: now,
                    leave_time: now,
                    ..Default::default()
                };

                self.services.voice_tracking.insert(&model).await?;
            }

            _ => {} // Same channel or other state changes (mute/deafen)
        }

        Ok(())
    }
}
