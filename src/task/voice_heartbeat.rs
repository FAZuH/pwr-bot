/// Heartbeat task for voice tracking crash recovery.
use std::sync::Arc;

use anyhow::Result;
use chrono::DateTime;
use chrono::Utc;
use log::debug;
use log::error;
use log::info;
use tokio::time::Duration;
use tokio::time::interval;

use crate::entity::BotMetaKey;
use crate::service::internal_service::InternalService;
use crate::service::voice_tracking_service::VoiceTrackingService;

/// Interval between heartbeats
const HEARTBEAT_INTERVAL_SECS: u64 = 10;

/// Manages heartbeat for voice tracking to prevent data loss on crashes.
pub struct VoiceHeartbeatManager {
    internal: Arc<InternalService>,
    service: Arc<VoiceTrackingService>,
}

impl VoiceHeartbeatManager {
    /// Creates a new heartbeat manager with the given service.
    pub fn new(internal: Arc<InternalService>, service: Arc<VoiceTrackingService>) -> Self {
        Self { internal, service }
    }

    /// Reads the last heartbeat timestamp from database.
    pub async fn read_last_heartbeat(&self) -> Result<Option<DateTime<Utc>>> {
        let value = self.internal.get_meta(BotMetaKey::VoiceHeartbeat).await?;

        match value {
            Some(ts_str) => {
                let timestamp = DateTime::parse_from_rfc3339(&ts_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .map_err(|e| anyhow::anyhow!("Invalid heartbeat timestamp: {}", e))?;
                Ok(Some(timestamp))
            }
            None => Ok(None),
        }
    }

    /// Starts the heartbeat task.
    pub async fn start(&self) {
        let internal = self.internal.clone();
        let service = self.service.clone();

        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(HEARTBEAT_INTERVAL_SECS));

            loop {
                interval.tick().await;
                let now = Utc::now();

                // Update all active sessions with current time as leave_time
                match service.find_active_sessions().await {
                    Ok(sessions) => {
                        let mut updated = 0;
                        for session in sessions {
                            if let Err(e) = service
                                .update_session_leave_time(
                                    session.user_id,
                                    session.channel_id,
                                    &session.join_time,
                                    &now,
                                )
                                .await
                            {
                                error!(
                                    "Failed to update heartbeat for user {}: {}",
                                    session.user_id, e
                                );
                            } else {
                                updated += 1;
                            }
                        }

                        if updated > 0 {
                            debug!("Heartbeat: Updated {} active voice sessions", updated);
                        }
                    }
                    Err(e) => {
                        error!("Failed to find active sessions for heartbeat: {}", e);
                    }
                }

                // Write heartbeat timestamp to database
                if let Err(e) = internal
                    .set_meta(BotMetaKey::VoiceHeartbeat, &now.to_rfc3339())
                    .await
                {
                    error!("Failed to write heartbeat to database: {}", e);
                } else {
                    debug!("Heartbeat written to database: {}", now);
                }
            }
        });

        info!(
            "Voice session heartbeat started (interval: {}s)",
            HEARTBEAT_INTERVAL_SECS
        );
    }

    /// Handles recovery from a crash by closing orphaned sessions.
    pub async fn recover_from_crash(&self) -> Result<u32> {
        let last_heartbeat = match self.read_last_heartbeat().await? {
            Some(ts) => ts,
            None => {
                info!("No previous heartbeat found, assuming clean shutdown");
                return Ok(0);
            }
        };

        info!(
            "Recovering from potential crash. Last heartbeat was at {}",
            last_heartbeat
        );

        // Find all active (unterminated) sessions
        let active_sessions = self.service.find_active_sessions().await?;
        let mut closed = 0u32;

        for session in active_sessions {
            // Use the last known heartbeat as the leave_time
            // This represents the last time the bot was known to be running
            // Also set is_active = 0 to properly close the session
            self.service
                .close_session(
                    session.user_id,
                    session.channel_id,
                    &session.join_time,
                    &last_heartbeat,
                )
                .await?;

            closed += 1;
            debug!(
                "Closed orphaned session for user {} in guild {} (duration: {}s)",
                session.user_id,
                session.guild_id,
                (last_heartbeat - session.join_time).num_seconds()
            );
        }

        info!(
            "Crash recovery complete: closed {} orphaned sessions",
            closed
        );
        Ok(closed)
    }
}
