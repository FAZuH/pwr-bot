//! Voice channel activity tracking service.

use std::collections::HashSet;
use std::sync::Arc;

use chrono::DateTime;
use chrono::Utc;
use tokio::sync::RwLock;

use crate::bot::commands::voice::GuildStatType;
use crate::model::GuildDailyStats;
use crate::model::ServerSettings;
use crate::model::VoiceDailyActivity;
use crate::model::VoiceLeaderboardEntry;
use crate::model::VoiceLeaderboardOpt;
use crate::model::VoiceSessionsModel;
use crate::repository::Repository;
use crate::repository::table::Table;
use crate::service::settings_service::SettingsService;

/// Service for tracking voice channel activity.
pub struct VoiceTrackingService {
    db: Arc<Repository>,
    settings: Arc<SettingsService>,
    disabled_guilds: Arc<RwLock<HashSet<u64>>>,
}

impl VoiceTrackingService {
    /// Creates a new voice tracking service and loads disabled guilds.
    pub async fn new(db: Arc<Repository>) -> anyhow::Result<Self> {
        let settings = Arc::new(SettingsService::new(db.clone()));
        let _self = Self {
            db,
            settings: settings.clone(),
            disabled_guilds: Arc::new(RwLock::new(HashSet::new())),
        };
        let all_settings = _self.db.server_settings.select_all().await?;
        let mut disabled = _self.disabled_guilds.write().await;

        for model in all_settings {
            if let Some(false) = model.settings.0.voice.enabled {
                disabled.insert(model.guild_id);
            }
        }
        drop(disabled);

        Ok(_self)
    }

    /// Check if voice tracking is enabled for a guild (default: true)
    pub async fn is_enabled(&self, guild_id: u64) -> bool {
        !self.disabled_guilds.read().await.contains(&guild_id)
    }

    pub async fn insert(&self, model: &VoiceSessionsModel) -> anyhow::Result<()> {
        self.db.voice_sessions.insert(model).await?;
        Ok(())
    }
    pub async fn replace(&self, model: &VoiceSessionsModel) -> anyhow::Result<()> {
        self.db.voice_sessions.replace(model).await?;
        Ok(())
    }

    pub async fn get_server_settings(&self, guild_id: u64) -> anyhow::Result<ServerSettings> {
        Ok(self.settings.get_server_settings(guild_id).await?)
    }

    pub async fn update_server_settings(
        &self,
        guild_id: u64,
        settings: ServerSettings,
    ) -> anyhow::Result<()> {
        // Update cache
        {
            let mut disabled = self.disabled_guilds.write().await;
            if let Some(false) = settings.voice.enabled {
                disabled.insert(guild_id);
            } else {
                disabled.remove(&guild_id);
            }
        }

        self.settings
            .update_server_settings(guild_id, settings)
            .await?;
        Ok(())
    }

    pub async fn get_leaderboard_withopt(
        &self,
        options: &VoiceLeaderboardOpt,
    ) -> anyhow::Result<Vec<VoiceLeaderboardEntry>> {
        Ok(self.db.voice_sessions.get_leaderboard_opt(options).await?)
    }

    pub async fn get_partner_leaderboard(
        &self,
        options: &VoiceLeaderboardOpt,
        target_user_id: u64,
    ) -> anyhow::Result<Vec<VoiceLeaderboardEntry>> {
        Ok(self
            .db
            .voice_sessions
            .get_partner_leaderboard(options, target_user_id)
            .await?)
    }

    pub async fn get_leaderboard(
        &self,
        guild_id: u64,
        limit: u32,
    ) -> anyhow::Result<Vec<VoiceLeaderboardEntry>> {
        Ok(self
            .db
            .voice_sessions
            .get_leaderboard(guild_id, limit)
            .await?)
    }

    pub async fn get_leaderboard_with_offset(
        &self,
        guild_id: u64,
        offset: u32,
        limit: u32,
    ) -> anyhow::Result<Vec<VoiceLeaderboardEntry>> {
        Ok(self
            .db
            .voice_sessions
            .get_leaderboard_with_offset(guild_id, offset, limit)
            .await?)
    }

    pub async fn get_voice_user_count(
        &self,
        _guild_id: impl Into<u64>,
        _from: &DateTime<Utc>,
        _until: &DateTime<Utc>,
    ) -> anyhow::Result<u32> {
        todo!()
    }

    /// Update leave_time for a specific session (heartbeat mechanism)
    pub async fn update_session_leave_time(
        &self,
        user_id: u64,
        channel_id: u64,
        join_time: &DateTime<Utc>,
        leave_time: &DateTime<Utc>,
    ) -> anyhow::Result<()> {
        self.db
            .voice_sessions
            .update_leave_time(user_id, channel_id, join_time, leave_time)
            .await?;
        Ok(())
    }

    /// Find all active sessions from database
    pub async fn find_active_sessions(&self) -> anyhow::Result<Vec<VoiceSessionsModel>> {
        Ok(self.db.voice_sessions.find_active_sessions().await?)
    }

    pub async fn get_sessions_in_range(
        &self,
        guild_id: u64,
        user_id: Option<u64>,
        since: &DateTime<Utc>,
        until: &DateTime<Utc>,
    ) -> anyhow::Result<Vec<VoiceSessionsModel>> {
        Ok(self
            .db
            .voice_sessions
            .get_sessions_in_range(guild_id, user_id, since, until)
            .await?)
    }

    /// Get daily voice activity for a specific user in a guild.
    pub async fn get_user_daily_activity(
        &self,
        user_id: u64,
        guild_id: u64,
        since: &DateTime<Utc>,
        until: &DateTime<Utc>,
    ) -> anyhow::Result<Vec<VoiceDailyActivity>> {
        Ok(self
            .db
            .voice_sessions
            .get_user_daily_activity(user_id, guild_id, since, until)
            .await?)
    }

    /// Get guild-wide daily statistics.
    pub async fn get_guild_daily_stats(
        &self,
        guild_id: u64,
        since: &DateTime<Utc>,
        until: &DateTime<Utc>,
        stat_type: GuildStatType,
    ) -> anyhow::Result<Vec<GuildDailyStats>> {
        match stat_type {
            GuildStatType::AverageTime => Ok(self
                .db
                .voice_sessions
                .get_guild_daily_average_time(guild_id, since, until)
                .await?),
            GuildStatType::ActiveUserCount => Ok(self
                .db
                .voice_sessions
                .get_guild_daily_user_count(guild_id, since, until)
                .await?),
            GuildStatType::TotalTime => Ok(self
                .db
                .voice_sessions
                .get_guild_daily_total_time(guild_id, since, until)
                .await?),
        }
    }
}

// pub struct VoiceTotalMemberData {
//     user_id: UserId,
//     name: String,
//     duration: Duration,
//     from: DateTime<Utc>,
//     until: DateTime<Utc>,
// }
