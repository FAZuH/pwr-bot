//! Voice-state event subscriber and session lifecycle rules.

use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Result;
use chrono::Utc;
use log::debug;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use tokio::sync::Mutex;

use crate::VoiceSessionsEntity;
use crate::service::VoiceTrackingService;

fn id_value<'de, D>(deserializer: D) -> std::result::Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Id {
        Number(u64),
        Text(String),
    }
    match Id::deserialize(deserializer)? {
        Id::Number(value) => Ok(value),
        Id::Text(value) => value.parse().map_err(serde::de::Error::custom),
    }
}

/// The voice-state fields used by the session lifecycle.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VoiceState {
    #[serde(deserialize_with = "id_value")]
    pub user_id: u64,
    #[serde(default, deserialize_with = "option_id_value")]
    pub guild_id: Option<u64>,
    #[serde(default, deserialize_with = "option_id_value")]
    pub channel_id: Option<u64>,
    pub session_id: String,
}

fn option_id_value<'de, D>(deserializer: D) -> std::result::Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Id {
        Number(u64),
        Text(String),
        Null,
    }
    match Id::deserialize(deserializer)? {
        Id::Number(value) => Ok(Some(value)),
        Id::Text(value) => value.parse().map(Some).map_err(serde::de::Error::custom),
        Id::Null => Ok(None),
    }
}

fn json_id(value: Option<&Value>) -> Option<u64> {
    value.and_then(|value| {
        value
            .as_u64()
            .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
    })
}

fn json_values(value: Option<&Value>) -> Vec<&Value> {
    match value {
        Some(Value::Array(values)) => values.iter().collect(),
        Some(Value::Object(values)) => values.values().collect(),
        _ => Vec::new(),
    }
}

/// A gateway voice-state transition delivered to the plugin.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VoiceStateEvent {
    /// The state before the gateway transition, if one existed.
    pub old: Option<VoiceState>,
    /// The state after the gateway transition.
    pub new: VoiceState,
}

/// Tracks voice sessions from gateway state events and guild snapshots.
pub struct VoiceStateSubscriber {
    service: Arc<VoiceTrackingService>,
    active_sessions: Mutex<HashSet<String>>,
}

impl VoiceStateSubscriber {
    /// Creates a subscriber backed by the plugin-owned voice service.
    pub fn new(service: Arc<VoiceTrackingService>) -> Self {
        Self {
            service,
            active_sessions: Mutex::new(HashSet::new()),
        }
    }

    async fn reserve_session(&self, session_id: &str) -> bool {
        self.active_sessions
            .lock()
            .await
            .insert(session_id.to_string())
    }

    async fn release_session(&self, session_id: &str) {
        self.active_sessions.lock().await.remove(session_id);
    }

    async fn replace_session(&self, old_session_id: &str, new_session_id: &str) -> bool {
        let mut active_sessions = self.active_sessions.lock().await;
        active_sessions.remove(old_session_id);
        active_sessions.insert(new_session_id.to_string())
    }

    async fn close_orphaned_sessions(&self, user_id: u64, guild_id: u64) -> Result<()> {
        let now = Utc::now();
        let sessions = self
            .service
            .find_active_sessions_by_user(user_id, guild_id)
            .await?;
        for session in sessions {
            self.service
                .close_session(
                    session.user_id,
                    session.channel_id,
                    &session.join_time,
                    &now,
                )
                .await?;
            debug!(
                "closed orphaned session for user {} in channel {} (guild {})",
                session.user_id, session.channel_id, session.guild_id
            );
        }
        Ok(())
    }

    /// Records a user already present in a voice channel when a guild snapshot arrives.
    pub async fn track_existing_user(
        &self,
        user_id: u64,
        guild_id: u64,
        channel_id: u64,
        session_id: &str,
    ) -> Result<()> {
        if !self.reserve_session(session_id).await {
            return Ok(());
        }
        let now = Utc::now();
        let result: Result<()> = async {
            self.close_orphaned_sessions(user_id, guild_id).await?;
            self.service
                .insert(&VoiceSessionsEntity {
                    user_id,
                    guild_id,
                    channel_id,
                    join_time: now,
                    leave_time: now,
                    is_active: true,
                    ..Default::default()
                })
                .await?;
            Ok(())
        }
        .await;
        if result.is_err() {
            self.release_session(session_id).await;
        } else {
            debug!("started tracking existing user {user_id} in voice channel {channel_id}");
        }
        result
    }

    async fn handle_join(&self, event: &VoiceStateEvent, channel_id: u64) -> Result<()> {
        let guild_id = event
            .new
            .guild_id
            .ok_or_else(|| anyhow::anyhow!("missing guild_id"))?;
        let session_id = event.new.session_id.clone();
        if !self.reserve_session(&session_id).await {
            return Ok(());
        }
        let now = Utc::now();
        let result: Result<()> = async {
            self.close_orphaned_sessions(event.new.user_id, guild_id)
                .await?;
            self.service
                .insert(&VoiceSessionsEntity {
                    user_id: event.new.user_id,
                    guild_id,
                    channel_id,
                    join_time: now,
                    leave_time: now,
                    is_active: true,
                    ..Default::default()
                })
                .await?;
            Ok(())
        }
        .await;
        if result.is_err() {
            self.release_session(&session_id).await;
        }
        result
    }

    async fn handle_leave(&self, event: &VoiceStateEvent) -> Result<()> {
        let old = event
            .old
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("missing old voice state"))?;
        let guild_id = old
            .guild_id
            .or(event.new.guild_id)
            .ok_or_else(|| anyhow::anyhow!("missing guild_id"))?;
        self.active_sessions.lock().await.remove(&old.session_id);
        let now = Utc::now();
        for session in self
            .service
            .find_active_sessions_by_user(old.user_id, guild_id)
            .await?
        {
            self.service
                .close_session(
                    session.user_id,
                    session.channel_id,
                    &session.join_time,
                    &now,
                )
                .await?;
        }
        Ok(())
    }

    async fn handle_move(&self, event: &VoiceStateEvent, new_channel_id: u64) -> Result<()> {
        let old = event
            .old
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("missing old voice state"))?;
        let guild_id = event
            .new
            .guild_id
            .ok_or_else(|| anyhow::anyhow!("missing guild_id"))?;
        if !self
            .replace_session(&old.session_id, &event.new.session_id)
            .await
        {
            return Ok(());
        }
        let now = Utc::now();
        let result: Result<()> = async {
            for session in self
                .service
                .find_active_sessions_by_user(event.new.user_id, guild_id)
                .await?
            {
                self.service
                    .close_session(
                        session.user_id,
                        session.channel_id,
                        &session.join_time,
                        &now,
                    )
                    .await?;
            }
            self.service
                .insert(&VoiceSessionsEntity {
                    user_id: event.new.user_id,
                    guild_id,
                    channel_id: new_channel_id,
                    join_time: now,
                    leave_time: now,
                    is_active: true,
                    ..Default::default()
                })
                .await?;
            Ok(())
        }
        .await;
        if result.is_err() {
            self.release_session(&event.new.session_id).await;
        }
        result
    }

    /// Applies a raw guild snapshot while filtering bots and disconnected states.
    /// The host owns event fan-out; the plugin owns this snapshot policy.
    pub async fn callback_guild_create(&self, data: &Value) -> Result<()> {
        let guild = data.get("guild").unwrap_or(data);
        let Some(guild_id) = json_id(guild.get("id")) else {
            return Ok(());
        };
        if !self.service.is_enabled(guild_id).await {
            return Ok(());
        }
        let bot_ids: HashSet<u64> = json_values(guild.get("members"))
            .into_iter()
            .filter_map(|member| member.get("user"))
            .filter(|user| user.get("bot").and_then(Value::as_bool) == Some(true))
            .filter_map(|user| json_id(user.get("id")))
            .collect();
        let states = json_values(guild.get("voice_states"));
        for state in states {
            let Some(user_id) = json_id(state.get("user_id")) else {
                continue;
            };
            if bot_ids.contains(&user_id) {
                continue;
            }
            let Some(channel_id) = json_id(state.get("channel_id")) else {
                continue;
            };
            let Some(session_id) = state.get("session_id").and_then(Value::as_str) else {
                continue;
            };
            self.track_existing_user(user_id, guild_id, channel_id, session_id)
                .await?;
        }
        Ok(())
    }

    /// Applies a voice-state transition to the session lifecycle.
    ///
    /// `old` is absent for a join, and `new.channel_id` is absent for a
    /// leave. A transition with both channel ids moves the session between
    /// channels; other state changes are ignored.
    pub async fn callback(&self, event: VoiceStateEvent) -> Result<()> {
        let guild_id = event
            .new
            .guild_id
            .or(event.old.as_ref().and_then(|old| old.guild_id));
        if let Some(guild_id) = guild_id
            && !self.service.is_enabled(guild_id).await
        {
            return Ok(());
        }
        match (
            event.old.as_ref().and_then(|old| old.channel_id),
            event.new.channel_id,
        ) {
            (None, Some(channel_id)) => self.handle_join(&event, channel_id).await,
            (Some(_), None) => self.handle_leave(&event).await,
            (Some(old_channel), Some(new_channel)) if old_channel != new_channel => {
                self.handle_move(&event, new_channel).await
            }
            _ => Ok(()),
        }
    }
}
