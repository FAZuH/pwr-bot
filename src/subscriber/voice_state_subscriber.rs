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

/// Tracks active voice sessions with their join times
#[derive(Clone, Debug)]
#[allow(dead_code)]
struct ActiveSession {
    user_id: u64,
    guild_id: u64,
    channel_id: u64,
    join_time: DateTime<Utc>,
}

pub struct VoiceStateSubscriber {
    pub services: Arc<Services>,
    active_sessions: Mutex<HashMap<String, ActiveSession>>,
}

impl VoiceStateSubscriber {
    pub fn new(services: Arc<Services>) -> Self {
        Self {
            services,
            active_sessions: Mutex::new(HashMap::new()),
        }
    }

    /// Start tracking a user who is already in a voice channel on bot startup.
    /// This is called for each user found in voice when the bot connects.
    pub async fn track_existing_user(
        &self,
        user_id: u64,
        guild_id: u64,
        channel_id: u64,
        session_id: &str,
    ) -> Result<()> {
        let now = Utc::now();
        let mut sessions = self.active_sessions.lock().await;

        // Only track if not already tracked
        if !sessions.contains_key(session_id) {
            let session = ActiveSession {
                user_id,
                guild_id,
                channel_id,
                join_time: now,
            };

            // Insert into database
            let model = VoiceSessionsModel {
                user_id,
                guild_id,
                channel_id,
                join_time: now,
                leave_time: now,
                ..Default::default()
            };

            self.services.voice_tracking.insert(&model).await?;
            sessions.insert(session_id.to_string(), session);

            debug!(
                "Started tracking existing user {} in voice channel {} (guild {})",
                user_id, channel_id, guild_id
            );
        }

        Ok(())
    }

    async fn handle_join(&self, event: &VoiceStateEvent, channel_id: ChannelId) -> Result<()> {
        debug!(
            "User {} detected joining voice channel id {}",
            event.new.user_id.get(),
            channel_id.get()
        );
        let join_time = Utc::now();
        let guild_id = event
            .new
            .guild_id
            .ok_or(anyhow::anyhow!("Missing guild_id"))?
            .get();
        let user_id = event.new.user_id.get();
        let session_id = event.new.session_id.to_string();

        let session = ActiveSession {
            user_id,
            guild_id,
            channel_id: channel_id.get(),
            join_time,
        };

        self.active_sessions
            .lock()
            .await
            .insert(session_id, session);

        let model = VoiceSessionsModel {
            user_id,
            guild_id,
            channel_id: channel_id.get(),
            join_time,
            leave_time: join_time,
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
        let session_id = old_state.session_id.to_string();

        let session = self.active_sessions.lock().await.remove(&session_id);

        if let Some(session) = session {
            let model = VoiceSessionsModel {
                user_id: old_state.user_id.get(),
                guild_id: old_state
                    .guild_id
                    .ok_or(anyhow::anyhow!("Missing guild_id"))?
                    .get(),
                channel_id: old_channel_id.get(),
                join_time: session.join_time,
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
        let session_id = old_state.session_id.to_string();

        // Close old session
        if let Some(session) = self.active_sessions.lock().await.remove(&session_id) {
            let model = VoiceSessionsModel {
                user_id: old_state.user_id.get(),
                guild_id: old_state
                    .guild_id
                    .ok_or(anyhow::anyhow!("Missing guild_id"))?
                    .get(),
                channel_id: old_channel_id.get(),
                join_time: session.join_time,
                leave_time: now,
                ..Default::default()
            };
            self.services.voice_tracking.replace(&model).await?;
        }

        // Start new session
        let guild_id = event
            .new
            .guild_id
            .ok_or(anyhow::anyhow!("Missing guild_id"))?
            .get();
        let user_id = event.new.user_id.get();

        let session = ActiveSession {
            user_id,
            guild_id,
            channel_id: new_channel_id.get(),
            join_time: now,
        };

        self.active_sessions
            .lock()
            .await
            .insert(session_id.clone(), session);

        let model = VoiceSessionsModel {
            user_id,
            guild_id,
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

        let sessions = sub.active_sessions.lock().await;
        assert!(sessions.contains_key("session1"));
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_handle_leave_logic() {
        let sub = create_mock_subscriber().await.unwrap();
        let join_time = Utc::now();
        let session = ActiveSession {
            user_id: 123,
            guild_id: 456,
            channel_id: 789,
            join_time,
        };
        sub.active_sessions
            .lock()
            .await
            .insert("session1".to_string(), session);

        let old_state = create_voice_state(123, Some(456), Some(789), "session1");
        let event = VoiceStateEvent {
            old: Some(old_state),
            new: create_voice_state(123, Some(456), None, "session1"),
        };

        let result = sub.handle_leave(&event, ChannelId::new(789)).await;
        assert!(result.is_ok());

        let sessions = sub.active_sessions.lock().await;
        assert!(!sessions.contains_key("session1"));
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_handle_move_logic() {
        let sub = create_mock_subscriber().await.unwrap();
        let join_time = Utc::now();
        let session = ActiveSession {
            user_id: 123,
            guild_id: 456,
            channel_id: 781,
            join_time,
        };
        sub.active_sessions
            .lock()
            .await
            .insert("session1".to_string(), session);

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

        let sessions = sub.active_sessions.lock().await;
        assert!(sessions.contains_key("session1"));
        assert_ne!(sessions.get("session1").unwrap().join_time, join_time);
        assert_eq!(sessions.get("session1").unwrap().channel_id, 782);
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_track_existing_user() {
        let sub = create_mock_subscriber().await.unwrap();

        // Track an existing user (simulating startup scan)
        let result = sub.track_existing_user(123, 456, 789, "session1").await;
        assert!(result.is_ok());

        // Verify session is tracked in memory
        let sessions = sub.active_sessions.lock().await;
        assert!(sessions.contains_key("session1"));
        assert_eq!(sessions.get("session1").unwrap().user_id, 123);
        assert_eq!(sessions.get("session1").unwrap().guild_id, 456);
        assert_eq!(sessions.get("session1").unwrap().channel_id, 789);
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_track_existing_user_already_tracked() {
        let sub = create_mock_subscriber().await.unwrap();

        // Track user first time
        sub.track_existing_user(123, 456, 789, "session1")
            .await
            .unwrap();

        let first_join_time = {
            let sessions = sub.active_sessions.lock().await;
            sessions.get("session1").unwrap().join_time
        };

        // Try to track same user again (should not create duplicate)
        sub.track_existing_user(123, 456, 789, "session1")
            .await
            .unwrap();

        // Verify still only one session and join_time hasn't changed
        let sessions = sub.active_sessions.lock().await;
        assert!(sessions.contains_key("session1"));
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions.get("session1").unwrap().join_time, first_join_time);
    }
}
