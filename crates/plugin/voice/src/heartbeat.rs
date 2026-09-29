//! File-backed heartbeat for voice-session crash recovery.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use chrono::DateTime;
use chrono::Utc;
use log::debug;
use log::error;
use log::info;
use tokio::time::Duration;
use tokio::time::interval;

use crate::service::VoiceTrackingService;

const HEARTBEAT_INTERVAL_SECS: u64 = 10;

/// Maintains the last-known-alive marker for voice tracking.
pub struct VoiceHeartbeatManager {
    path: PathBuf,
    service: Arc<VoiceTrackingService>,
}

impl VoiceHeartbeatManager {
    pub fn new(data_path: impl Into<PathBuf>, service: Arc<VoiceTrackingService>) -> Self {
        Self {
            path: data_path.into().join("voice_heartbeat"),
            service,
        }
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    pub async fn read_last_heartbeat(&self) -> anyhow::Result<Option<DateTime<Utc>>> {
        let path = self.path.clone();
        let value = match tokio::task::spawn_blocking(move || std::fs::read_to_string(path)).await {
            Ok(Ok(value)) => value,
            Ok(Err(error)) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Ok(Err(error)) => return Err(error.into()),
            Err(error) => return Err(error.into()),
        };
        if value.trim().is_empty() {
            return Ok(None);
        }
        let timestamp = DateTime::parse_from_rfc3339(value.trim())
            .map_err(|error| anyhow::anyhow!("invalid heartbeat timestamp: {error}"))?
            .with_timezone(&Utc);
        Ok(Some(timestamp))
    }

    pub async fn start(self: Arc<Self>) {
        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(HEARTBEAT_INTERVAL_SECS));
            loop {
                ticker.tick().await;
                self.update().await;
            }
        });
        info!("voice session heartbeat started (interval: {HEARTBEAT_INTERVAL_SECS}s)");
    }

    pub async fn recover_from_crash(&self) -> anyhow::Result<u32> {
        let Some(last_heartbeat) = self.read_last_heartbeat().await? else {
            info!("no previous heartbeat found, assuming clean shutdown");
            return Ok(0);
        };
        info!("recovering from potential crash; last heartbeat was at {last_heartbeat}");
        let mut closed = 0;
        for session in self.service.find_active_sessions().await? {
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
                "closed orphaned session for user {} in guild {}",
                session.user_id, session.guild_id
            );
        }
        info!("crash recovery complete: closed {closed} orphaned sessions");
        Ok(closed)
    }

    pub async fn update(&self) {
        let path = self.path.clone();
        let value = Utc::now().to_rfc3339();
        let result = tokio::task::spawn_blocking(move || {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).context("create heartbeat directory")?;
            }
            std::fs::write(path, value).context("write heartbeat file")
        })
        .await;
        match result {
            Ok(Ok(())) => debug!("heartbeat written to file"),
            Ok(Err(error)) => error!("failed to write heartbeat file: {error}"),
            Err(error) => error!("heartbeat task failed: {error}"),
        }
    }
}
