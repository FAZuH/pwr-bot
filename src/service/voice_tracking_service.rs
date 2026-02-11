//! Voice channel activity tracking service.

use std::collections::HashSet;
use std::sync::Arc;

use chrono::DateTime;
use chrono::Utc;
use tokio::sync::RwLock;

use crate::database::Database;
use crate::database::model::ServerSettings;
use crate::database::model::ServerSettingsModel;
use crate::database::model::VoiceLeaderboardEntry;
use crate::database::model::VoiceSessionsModel;
use crate::database::table::Table;

/// Service for tracking voice channel activity.
pub struct VoiceTrackingService {
    db: Arc<Database>,
    disabled_guilds: Arc<RwLock<HashSet<u64>>>,
}

impl VoiceTrackingService {
    /// Creates a new voice tracking service and loads disabled guilds.
    pub async fn new(db: Arc<Database>) -> anyhow::Result<Self> {
        let _self = Self {
            db,
            disabled_guilds: Arc::new(RwLock::new(HashSet::new())),
        };
        let all_settings = _self.db.server_settings_table.select_all().await?;
        let mut disabled = _self.disabled_guilds.write().await;

        for model in all_settings {
            if let Some(false) = model.settings.0.voice_tracking_enabled {
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
        self.db.voice_sessions_table.insert(model).await?;
        Ok(())
    }
    pub async fn replace(&self, model: &VoiceSessionsModel) -> anyhow::Result<()> {
        self.db.voice_sessions_table.replace(model).await?;
        Ok(())
    }

    pub async fn get_server_settings(&self, guild_id: u64) -> anyhow::Result<ServerSettings> {
        match self.db.server_settings_table.select(&guild_id).await? {
            Some(model) => Ok(model.settings.0),
            None => Ok(ServerSettings::default()),
        }
    }

    pub async fn update_server_settings(
        &self,
        guild_id: u64,
        settings: ServerSettings,
    ) -> anyhow::Result<()> {
        // Update cache
        {
            let mut disabled = self.disabled_guilds.write().await;
            if let Some(false) = settings.voice_tracking_enabled {
                disabled.insert(guild_id);
            } else {
                disabled.remove(&guild_id);
            }
        }

        let model = ServerSettingsModel {
            guild_id,
            settings: sqlx::types::Json(settings),
        };
        self.db.server_settings_table.replace(&model).await?;
        Ok(())
    }

    pub async fn get_leaderboard(
        &self,
        guild_id: u64,
        limit: u32,
    ) -> anyhow::Result<Vec<VoiceLeaderboardEntry>> {
        Ok(self
            .db
            .voice_sessions_table
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
            .voice_sessions_table
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
            .voice_sessions_table
            .update_leave_time(user_id, channel_id, join_time, leave_time)
            .await?;
        Ok(())
    }

    /// Find all active sessions from database
    pub async fn find_active_sessions(&self) -> anyhow::Result<Vec<VoiceSessionsModel>> {
        Ok(self.db.voice_sessions_table.find_active_sessions().await?)
    }
}

// pub struct VoiceTotalMemberData {
//     user_id: UserId,
//     name: String,
//     duration: Duration,
//     from: DateTime<Utc>,
//     until: DateTime<Utc>,
// }
