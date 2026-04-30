//! Subscriber that tracks voice channel state changes.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use chrono::DateTime;
use chrono::Utc;
use log::debug;
use poise::serenity_prelude::ChannelId;
use tokio::sync::Mutex;

use crate::entity::VoiceSessionsEntity;
use crate::event::VoiceStateEvent;
use crate::service::Services;
use crate::subscriber::Subscriber;

/// Tracks active voice sessions with their join times.
#[derive(Clone, Debug)]
#[allow(dead_code)]
struct ActiveSession {
    user_id: u64,
    guild_id: u64,
    channel_id: u64,
    join_time: DateTime<Utc>,
}

/// Subscriber that tracks voice channel state changes.
pub struct VoiceStateSubscriber {
    pub services: Arc<Services>,
    active_sessions: Mutex<HashMap<String, ActiveSession>>,
}

impl VoiceStateSubscriber {
    /// Creates a new voice state subscriber.
    pub fn new(services: Arc<Services>) -> Self {
        Self {
            services,
            active_sessions: Mutex::new(HashMap::new()),
        }
    }

    /// Closes all orphaned active sessions for a user in a guild.
    async fn close_orphaned_sessions(&self, user_id: u64, guild_id: u64) -> Result<()> {
        let now = Utc::now();
        let orphaned = self
            .services
            .voice_tracking
            .find_active_sessions_by_user(user_id, guild_id)
            .await?;

        for session in orphaned {
            self.services
                .voice_tracking
                .close_session(
                    session.user_id,
                    session.channel_id,
                    &session.join_time,
                    &now,
                )
                .await?;
            debug!(
                "Closed orphaned session for user {} in channel {} (guild {})",
                session.user_id, session.channel_id, session.guild_id
            );
        }

        Ok(())
    }

    /// Tracks an existing user in a voice channel (used on bot startup).
    pub async fn track_existing_user(
        &self,
        user_id: u64,
        guild_id: u64,
        channel_id: u64,
        session_id: &str,
    ) -> Result<()> {
        let now = Utc::now();
        let sessions = self.active_sessions.lock().await;

        // Only track if not already tracked
        if !sessions.contains_key(session_id) {
            // Close any orphaned active sessions before creating a new one
            drop(sessions);
            self.close_orphaned_sessions(user_id, guild_id).await?;
            let mut sessions = self.active_sessions.lock().await;

            let session = ActiveSession {
                user_id,
                guild_id,
                channel_id,
                join_time: now,
            };

            // Insert into database
            let model = VoiceSessionsEntity {
                user_id,
                guild_id,
                channel_id,
                join_time: now,
                leave_time: now,
                is_active: true,
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

        // Skip if already tracking this session (prevents duplicates on gateway reconnects)
        if self.active_sessions.lock().await.contains_key(&session_id) {
            return Ok(());
        }

        // Close any orphaned active sessions before creating a new one
        self.close_orphaned_sessions(user_id, guild_id).await?;

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

        let model = VoiceSessionsEntity {
            user_id,
            guild_id,
            channel_id: channel_id.get(),
            join_time,
            leave_time: join_time,
            is_active: true,
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
        let user_id = old_state.user_id.get();
        let guild_id = old_state.guild_id.map(|g| g.get()).unwrap_or(0);

        // Remove from in-memory tracking
        self.active_sessions.lock().await.remove(&session_id);

        // Close ALL active sessions for this user in the DB
        // (not just the one tracked in memory, to handle orphaned sessions)
        let active_sessions = self
            .services
            .voice_tracking
            .find_active_sessions_by_user(user_id, guild_id)
            .await?;

        for session in active_sessions {
            self.services
                .voice_tracking
                .close_session(
                    session.user_id,
                    session.channel_id,
                    &session.join_time,
                    &leave_time,
                )
                .await?;
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
        let old_session_id = old_state.session_id.to_string();
        let new_session_id = event.new.session_id.to_string();
        let user_id = event.new.user_id.get();
        let guild_id = event
            .new
            .guild_id
            .ok_or(anyhow::anyhow!("Missing guild_id"))?
            .get();

        // Remove old session from in-memory tracking
        self.active_sessions.lock().await.remove(&old_session_id);

        // Close ALL active sessions for this user in the DB
        // (handles orphaned sessions from previous crashes)
        let active_sessions = self
            .services
            .voice_tracking
            .find_active_sessions_by_user(user_id, guild_id)
            .await?;

        for session in active_sessions {
            self.services
                .voice_tracking
                .close_session(
                    session.user_id,
                    session.channel_id,
                    &session.join_time,
                    &now,
                )
                .await?;
        }

        // Start new session
        let session = ActiveSession {
            user_id,
            guild_id,
            channel_id: new_channel_id.get(),
            join_time: now,
        };

        self.active_sessions
            .lock()
            .await
            .insert(new_session_id, session);

        let model = VoiceSessionsEntity {
            user_id,
            guild_id,
            channel_id: new_channel_id.get(),
            join_time: now,
            leave_time: now,
            is_active: true,
            ..Default::default()
        };

        self.services.voice_tracking.insert(&model).await?;
        Ok(())
    }
}

#[async_trait::async_trait]
impl Subscriber<VoiceStateEvent> for VoiceStateSubscriber {
    /// From <https://discord.com/developers/docs/events/gateway-events#voice-state-update>:
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
    use poise::serenity_prelude::VoiceState;

    use super::*;
    use crate::feed::Platforms;
    use crate::repo::Repository;

    async fn create_mock_subscriber() -> anyhow::Result<VoiceStateSubscriber> {
        let db_url = std::env::var("DB_URL")
            .unwrap_or("postgres://pwr_bot:pwr_bot@localhost:5432/pwr_bot".to_string());

        let db = Repository::new(&db_url).await.unwrap();
        db.run_migrations().await.unwrap();

        // Clean voice_sessions table to ensure test isolation
        use diesel_async::RunQueryDsl;
        let mut conn = db.pool().get().await.unwrap();
        diesel::sql_query("TRUNCATE TABLE voice_sessions RESTART IDENTITY CASCADE")
            .execute(&mut conn)
            .await
            .unwrap();

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

    #[tokio::test]
    #[serial_test::serial]
    async fn test_handle_join_closes_orphaned_sessions() {
        let sub = create_mock_subscriber().await.unwrap();
        let user_id = 444u64;
        let guild_id = 555u64;
        let channel_id = 789u64;

        // Simulate an orphaned active session (e.g., from a previous crash)
        let orphaned = VoiceSessionsEntity {
            id: 0,
            user_id,
            guild_id,
            channel_id,
            join_time: Utc::now() - chrono::Duration::hours(2),
            leave_time: Utc::now() - chrono::Duration::hours(2),
            is_active: true,
        };
        sub.services.voice_tracking.insert(&orphaned).await.unwrap();

        // Verify the orphaned session exists in the DB
        let active_before = sub
            .services
            .voice_tracking
            .find_active_sessions_by_user(user_id, guild_id)
            .await
            .unwrap();
        assert_eq!(active_before.len(), 1);

        // Now simulate the user joining voice (should close the orphaned session first)
        let event = VoiceStateEvent {
            old: None,
            new: create_voice_state(user_id, Some(guild_id), Some(channel_id), "session1"),
        };

        let result = sub.handle_join(&event, ChannelId::new(channel_id)).await;
        assert!(result.is_ok());

        // Verify the orphaned session was closed
        let active_after = sub
            .services
            .voice_tracking
            .find_active_sessions_by_user(user_id, guild_id)
            .await
            .unwrap();
        assert_eq!(active_after.len(), 1);

        // And the new session should be the one we just created
        assert!(active_after[0].join_time > orphaned.join_time);
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_handle_leave_closes_all_active_sessions() {
        let sub = create_mock_subscriber().await.unwrap();
        let user_id = 555u64;
        let guild_id = 666u64;

        // Create multiple active sessions (simulating duplicates from crashes)
        for channel_id in [789u64, 790, 791] {
            let session = VoiceSessionsEntity {
                id: 0,
                user_id,
                guild_id,
                channel_id,
                join_time: Utc::now() - chrono::Duration::hours(1),
                leave_time: Utc::now() - chrono::Duration::hours(1),
                is_active: true,
            };
            sub.services.voice_tracking.insert(&session).await.unwrap();
        }

        // Verify all 3 are active
        let active_before = sub
            .services
            .voice_tracking
            .find_active_sessions_by_user(user_id, guild_id)
            .await
            .unwrap();
        assert_eq!(active_before.len(), 3);

        // Now simulate the user leaving voice
        let old_state = create_voice_state(user_id, Some(guild_id), Some(789), "session1");
        let event = VoiceStateEvent {
            old: Some(old_state),
            new: create_voice_state(user_id, Some(guild_id), None, "session1"),
        };

        let result = sub.handle_leave(&event, ChannelId::new(789)).await;
        assert!(result.is_ok());

        // Verify ALL active sessions were closed
        let active_after = sub
            .services
            .voice_tracking
            .find_active_sessions_by_user(user_id, guild_id)
            .await
            .unwrap();
        assert!(active_after.is_empty());
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_handle_join_dedup_same_session_id() {
        let sub = create_mock_subscriber().await.unwrap();
        let user_id = 666u64;
        let guild_id = 777u64;
        let channel_id = 888u64;

        let event = VoiceStateEvent {
            old: None,
            new: create_voice_state(user_id, Some(guild_id), Some(channel_id), "session_dup"),
        };

        // First join should succeed
        let result = sub.handle_join(&event, ChannelId::new(channel_id)).await;
        assert!(result.is_ok());

        let active_after_first = sub
            .services
            .voice_tracking
            .find_active_sessions_by_user(user_id, guild_id)
            .await
            .unwrap();
        assert_eq!(active_after_first.len(), 1);

        // Second join with same session_id should be a no-op
        let result = sub.handle_join(&event, ChannelId::new(channel_id)).await;
        assert!(result.is_ok());

        let active_after_second = sub
            .services
            .voice_tracking
            .find_active_sessions_by_user(user_id, guild_id)
            .await
            .unwrap();
        assert_eq!(active_after_second.len(), 1);
        assert_eq!(
            active_after_second[0].join_time,
            active_after_first[0].join_time
        );
    }
}
