/// Heartbeat task for voice tracking crash recovery.
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use chrono::DateTime;
use chrono::Utc;
use log::debug;
use log::error;
use log::info;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio::time::Duration;
use tokio::time::interval;

use crate::service::voice_tracking_service::VoiceTrackingService;

/// File to store the last heartbeat timestamp.
const HEARTBEAT_FILE: &str = "voice_heartbeat.json";

/// Interval between heartbeats
const HEARTBEAT_INTERVAL_SECS: u64 = 10;

/// Data stored in the heartbeat file.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct HeartbeatData {
    timestamp: DateTime<Utc>,
    version: u32,
}

impl Default for HeartbeatData {
    fn default() -> Self {
        Self {
            timestamp: Utc::now(),
            version: 1,
        }
    }
}

/// Manages heartbeat for voice tracking to prevent data loss on crashes.
pub struct VoiceHeartbeatManager {
    data_dir: PathBuf,
    service: Arc<VoiceTrackingService>,
}

impl VoiceHeartbeatManager {
    /// Creates a new heartbeat manager with the given data directory.
    pub fn new(data_dir: impl Into<PathBuf>, service: Arc<VoiceTrackingService>) -> Self {
        Self {
            data_dir: data_dir.into(),
            service,
        }
    }

    /// Gets the path to the heartbeat file.
    fn heartbeat_file_path(&self) -> PathBuf {
        self.data_dir.join(HEARTBEAT_FILE)
    }

    /// Reads the last heartbeat timestamp from file.
    pub async fn read_last_heartbeat(&self) -> Result<Option<DateTime<Utc>>> {
        let path = self.heartbeat_file_path();

        if !path.exists() {
            return Ok(None);
        }

        let content = fs::read_to_string(&path).await?;
        let data: HeartbeatData = serde_json::from_str(&content)?;

        Ok(Some(data.timestamp))
    }

    /// Starts the heartbeat task.
    pub async fn start(&self) {
        let service = self.service.clone();
        let data_dir = self.data_dir.clone();

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

                // Write heartbeat timestamp to file
                let heartbeat_data = HeartbeatData {
                    timestamp: now,
                    version: 1,
                };
                let path = data_dir.join(HEARTBEAT_FILE);
                if let Ok(content) = serde_json::to_string_pretty(&heartbeat_data)
                    && let Ok(mut file) = fs::File::create(&path).await
                    && file.write_all(content.as_bytes()).await.is_ok()
                    && file.sync_all().await.is_ok()
                {
                    debug!("Heartbeat written to file: {}", now);
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
            self.service
                .update_session_leave_time(
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
