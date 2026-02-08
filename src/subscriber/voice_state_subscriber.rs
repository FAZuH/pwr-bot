use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use chrono::DateTime;
use chrono::Utc;
use log::debug;
use serenity::all::ChannelId;
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

    async fn handle_join(&self, event: &VoiceStateEvent, channel_id: ChannelId) -> Result<()> {
        debug!(
            "User {} detected joining voice channel id {}",
            event.new.user_id.get(),
            channel_id.get()
        );
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
        Ok(())
    }

    async fn handle_leave(&self, event: &VoiceStateEvent, old_channel_id: ChannelId) -> Result<()> {
        debug!(
            "User {} detected leaving voice channel id {}",
            event.new.user_id.get(),
            old_channel_id.get()
        );
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
        Ok(())
    }

    async fn handle_move(
        &self,
        event: &VoiceStateEvent,
        old_channel_id: ChannelId,
        new_channel_id: ChannelId,
    ) -> Result<()> {
        debug!(
            "User {} detected moving from voice channel id {} to {}",
            event.new.user_id.get(),
            old_channel_id.get(),
            new_channel_id.get()
        );
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
            .insert(event.new.session_id.to_string(), now);

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
        Ok(())
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
        let guild_id = event
            .new
            .guild_id
            .or_else(|| event.old.as_ref().and_then(|v| v.guild_id));

        if let Some(guild_id) = guild_id
            && !self
                .services
                .voice_tracking
                .is_enabled(guild_id.get())
                .await
        {
            return Ok(());
        }

        let old_channel = event.old.as_ref().and_then(|v| v.channel_id);
        let new_channel = event.new.channel_id;

        match (old_channel, new_channel) {
            // User joined
            (None, Some(channel_id)) => self.handle_join(&event, channel_id).await?,

            // User left
            (Some(old_channel_id), None) => self.handle_leave(&event, old_channel_id).await?,

            // User moved channels
            (Some(old_channel_id), Some(new_channel_id)) if old_channel_id != new_channel_id => {
                self.handle_move(&event, old_channel_id, new_channel_id)
                    .await?
            }

            _ => {} // Same channel or other state changes (mute/deafen)
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use serenity::all::VoiceState;

    use super::*;
    use crate::database::Database;
    use crate::feed::platforms::Platforms;

    async fn create_mock_subscriber() -> anyhow::Result<VoiceStateSubscriber> {
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let db_path = format!("/tmp/pwr-bot-test-{t}.sqlite");
        let db_url = format!("sqlite://{db_path}");

        let db = Database::new(&db_url, &db_path).await.unwrap();
        db.run_migrations().await.unwrap();
        let services = Arc::new(Services::new(Arc::new(db), Arc::new(Platforms::new())).await?);
        Ok(VoiceStateSubscriber::new(services))
    }

    fn create_voice_state(
        user_id: u64,
        guild_id: Option<u64>,
        channel_id: Option<u64>,
        session_id: &str,
    ) -> VoiceState {
        let json = serde_json::json!({
            "user_id": user_id.to_string(),
            "guild_id": guild_id.map(|id| id.to_string()),
            "channel_id": channel_id.map(|id| id.to_string()),
            "session_id": session_id,
            "deaf": false,
            "mute": false,
            "self_deaf": false,
            "self_mute": false,
            "suppress": false,
            "self_video": false,
        });
        serde_json::from_value(json).unwrap()
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_handle_join_logic() {
        let sub = create_mock_subscriber().await.unwrap();
        let event = VoiceStateEvent {
            old: None,
            new: create_voice_state(123, Some(456), Some(789), "session1"),
        };

        let result = sub.handle_join(&event, ChannelId::new(789)).await;
        assert!(result.is_ok());

        let joins = sub.session_joins.lock().await;
        assert!(joins.contains_key("session1"));
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_handle_leave_logic() {
        let sub = create_mock_subscriber().await.unwrap();
        let join_time = Utc::now();
        sub.session_joins
            .lock()
            .await
            .insert("session1".to_string(), join_time);

        let old_state = create_voice_state(123, Some(456), Some(789), "session1");
        let event = VoiceStateEvent {
            old: Some(old_state),
            new: create_voice_state(123, Some(456), None, "session1"),
        };

        let result = sub.handle_leave(&event, ChannelId::new(789)).await;
        assert!(result.is_ok());

        let joins = sub.session_joins.lock().await;
        assert!(!joins.contains_key("session1"));
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_handle_move_logic() {
        let sub = create_mock_subscriber().await.unwrap();
        let join_time = Utc::now();
        sub.session_joins
            .lock()
            .await
            .insert("session1".to_string(), join_time);

        let old_state = create_voice_state(123, Some(456), Some(781), "session1");
        let new_state = create_voice_state(123, Some(456), Some(782), "session1");
        let event = VoiceStateEvent {
            old: Some(old_state),
            new: new_state,
        };

        let result = sub
            .handle_move(&event, ChannelId::new(781), ChannelId::new(782))
            .await;
        assert!(result.is_ok());

        let joins = sub.session_joins.lock().await;
        assert!(joins.contains_key("session1"));
        assert_ne!(*joins.get("session1").unwrap(), join_time);
    }
}
