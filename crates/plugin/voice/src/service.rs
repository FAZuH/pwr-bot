//! Voice session persistence and query service.

use std::collections::HashSet;
use std::sync::Arc;

use chrono::DateTime;
use chrono::Utc;
use tokio::sync::RwLock;

use crate::GuildDailyStats;
use crate::GuildStatType;
use crate::VoiceDailyActivity;
use crate::VoiceLeaderboardEntry;
use crate::VoiceLeaderboardOpt;
use crate::VoiceSessionsEntity;
use crate::VoiceSettings;
use crate::repo::traits::VoiceSessionsRepository;
use crate::repo::traits::VoiceSettingsRepository;

/// Owns voice-session writes, queries, and the per-guild enabled cache.
pub struct VoiceTrackingService {
    voice_sessions: Arc<dyn VoiceSessionsRepository>,
    voice_settings: Arc<dyn VoiceSettingsRepository>,
    disabled_guilds: Arc<RwLock<HashSet<u64>>>,
}

impl VoiceTrackingService {
    /// Creates the service and loads the disabled-guild cache from storage.
    pub async fn new(
        voice_sessions: Arc<dyn VoiceSessionsRepository>,
        voice_settings: Arc<dyn VoiceSettingsRepository>,
    ) -> anyhow::Result<Self> {
        let service = Self {
            voice_sessions,
            voice_settings,
            disabled_guilds: Arc::new(RwLock::new(HashSet::new())),
        };
        let mut disabled = service.disabled_guilds.write().await;
        for settings in service.voice_settings.list_all().await? {
            if !settings.enabled {
                disabled.insert(settings.guild_id.into());
            }
        }
        drop(disabled);
        Ok(service)
    }

    /// Returns whether voice tracking is enabled for a guild.
    pub async fn is_enabled(&self, guild_id: u64) -> bool {
        !self.disabled_guilds.read().await.contains(&guild_id)
    }

    /// Inserts a new voice session.
    pub async fn insert(&self, model: &VoiceSessionsEntity) -> anyhow::Result<()> {
        self.voice_sessions.insert(model).await?;
        Ok(())
    }

    /// Replaces an existing voice session, or inserts it when absent.
    pub async fn replace(&self, model: &VoiceSessionsEntity) -> anyhow::Result<()> {
        self.voice_sessions.replace(model).await?;
        Ok(())
    }

    /// Reads the guild's voice settings, defaulting to enabled when absent.
    pub async fn get_settings(&self, guild_id: u64) -> anyhow::Result<VoiceSettings> {
        Ok(self
            .voice_settings
            .get(guild_id)
            .await?
            .map(|settings| VoiceSettings {
                enabled: settings.enabled,
            })
            .unwrap_or_default())
    }

    /// Persists voice settings and updates the in-memory guild cache.
    pub async fn update_settings(
        &self,
        guild_id: u64,
        settings: VoiceSettings,
    ) -> anyhow::Result<()> {
        self.voice_settings
            .replace(&crate::VoiceSettingsEntity {
                guild_id: guild_id.into(),
                enabled: settings.enabled,
            })
            .await?;
        let mut disabled = self.disabled_guilds.write().await;
        if settings.enabled {
            disabled.remove(&guild_id);
        } else {
            disabled.insert(guild_id);
        }
        Ok(())
    }

    /// Reads the server leaderboard using the supplied filters.
    pub async fn get_leaderboard_withopt(
        &self,
        options: &VoiceLeaderboardOpt,
    ) -> anyhow::Result<Vec<VoiceLeaderboardEntry>> {
        Ok(self.voice_sessions.get_leaderboard_opt(options).await?)
    }

    /// Reads a partner leaderboard for a target user.
    pub async fn get_partner_leaderboard(
        &self,
        options: &VoiceLeaderboardOpt,
        target_user_id: u64,
    ) -> anyhow::Result<Vec<VoiceLeaderboardEntry>> {
        Ok(self
            .voice_sessions
            .get_partner_leaderboard(options, target_user_id)
            .await?)
    }

    /// Updates a session's leave time for heartbeat recovery.
    pub async fn update_session_leave_time(
        &self,
        user_id: u64,
        channel_id: u64,
        join_time: &DateTime<Utc>,
        leave_time: &DateTime<Utc>,
    ) -> anyhow::Result<()> {
        self.voice_sessions
            .update_leave_time(user_id, channel_id, join_time, leave_time)
            .await?;
        Ok(())
    }

    /// Closes a session by setting its leave time and active flag atomically.
    pub async fn close_session(
        &self,
        user_id: u64,
        channel_id: u64,
        join_time: &DateTime<Utc>,
        leave_time: &DateTime<Utc>,
    ) -> anyhow::Result<()> {
        self.voice_sessions
            .close_session(user_id, channel_id, join_time, leave_time)
            .await?;
        Ok(())
    }

    /// Finds all active sessions for heartbeat recovery.
    pub async fn find_active_sessions(&self) -> anyhow::Result<Vec<VoiceSessionsEntity>> {
        Ok(self.voice_sessions.find_active_sessions().await?)
    }

    /// Finds active sessions for one user in a guild.
    pub async fn find_active_sessions_by_user(
        &self,
        user_id: u64,
        guild_id: u64,
    ) -> anyhow::Result<Vec<VoiceSessionsEntity>> {
        Ok(self
            .voice_sessions
            .find_active_sessions_by_user(user_id, guild_id)
            .await?)
    }

    /// Reads sessions for a guild and optional user in a time range.
    pub async fn get_sessions_in_range(
        &self,
        guild_id: u64,
        user_id: Option<u64>,
        since: &DateTime<Utc>,
        until: &DateTime<Utc>,
    ) -> anyhow::Result<Vec<VoiceSessionsEntity>> {
        Ok(self
            .voice_sessions
            .get_sessions_in_range(guild_id, user_id, since, until)
            .await?)
    }

    /// Reads daily voice activity for a user in a guild.
    pub async fn get_user_daily_activity(
        &self,
        user_id: u64,
        guild_id: u64,
        since: &DateTime<Utc>,
        until: &DateTime<Utc>,
    ) -> anyhow::Result<Vec<VoiceDailyActivity>> {
        Ok(self
            .voice_sessions
            .get_user_daily_activity(user_id, guild_id, since, until)
            .await?)
    }

    /// Reads guild-wide daily statistics for a selected metric.
    pub async fn get_guild_daily_stats(
        &self,
        guild_id: u64,
        since: &DateTime<Utc>,
        until: &DateTime<Utc>,
        stat_type: GuildStatType,
    ) -> anyhow::Result<Vec<GuildDailyStats>> {
        Ok(self
            .voice_sessions
            .get_guild_daily_stats(guild_id, since, until, stat_type)
            .await?)
    }
}
