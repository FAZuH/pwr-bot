use std::sync::Arc;
use std::sync::Mutex;

use async_trait::async_trait;
use chrono::DateTime;
use chrono::Duration;
use chrono::Utc;
use voice::GuildDailyStats;
use voice::GuildStatType;
use voice::VoiceDailyActivity;
use voice::VoiceLeaderboardEntry;
use voice::VoiceLeaderboardOpt;
use voice::VoiceSessionsEntity;
use voice::VoiceSettingsEntity;
use voice::heartbeat::VoiceHeartbeatManager;
use voice::repo::error::DatabaseError;
use voice::repo::traits::VoiceSessionsRepository;
use voice::repo::traits::VoiceSettingsRepository;
use voice::service::VoiceTrackingService;

struct MemorySessions {
    active: Mutex<Vec<VoiceSessionsEntity>>,
}

#[async_trait]
impl VoiceSessionsRepository for MemorySessions {
    async fn select_all(&self) -> Result<Vec<VoiceSessionsEntity>, DatabaseError> {
        Ok(self.active.lock().unwrap().clone())
    }

    async fn insert(&self, model: &VoiceSessionsEntity) -> Result<i32, DatabaseError> {
        self.active.lock().unwrap().push(model.clone());
        Ok(1)
    }

    async fn replace(&self, model: &VoiceSessionsEntity) -> Result<i32, DatabaseError> {
        self.active.lock().unwrap().push(model.clone());
        Ok(1)
    }

    async fn delete_all(&self) -> Result<(), DatabaseError> {
        self.active.lock().unwrap().clear();
        Ok(())
    }

    async fn get_leaderboard_opt(
        &self,
        _options: &VoiceLeaderboardOpt,
    ) -> Result<Vec<VoiceLeaderboardEntry>, DatabaseError> {
        Ok(Vec::new())
    }

    async fn get_partner_leaderboard(
        &self,
        _options: &VoiceLeaderboardOpt,
        _target_user_id: u64,
    ) -> Result<Vec<VoiceLeaderboardEntry>, DatabaseError> {
        Ok(Vec::new())
    }

    async fn update_leave_time(
        &self,
        user_id: u64,
        channel_id: u64,
        join_time: &DateTime<Utc>,
        leave_time: &DateTime<Utc>,
    ) -> Result<(), DatabaseError> {
        let mut active = self.active.lock().unwrap();
        if let Some(session) = active.iter_mut().find(|session| {
            session.user_id == user_id
                && session.channel_id == channel_id
                && session.join_time == *join_time
                && session.is_active
        }) {
            session.is_active = false;
            session.leave_time = *leave_time;
        }
        Ok(())
    }

    async fn close_session(
        &self,
        user_id: u64,
        _channel_id: u64,
        _join_time: &DateTime<Utc>,
        leave_time: &DateTime<Utc>,
    ) -> Result<(), DatabaseError> {
        let mut active = self.active.lock().unwrap();
        if let Some(session) = active
            .iter_mut()
            .find(|session| session.user_id == user_id && session.is_active)
        {
            session.is_active = false;
            session.leave_time = *leave_time;
        }
        Ok(())
    }

    async fn find_active_sessions(&self) -> Result<Vec<VoiceSessionsEntity>, DatabaseError> {
        Ok(self
            .active
            .lock()
            .unwrap()
            .iter()
            .filter(|session| session.is_active)
            .cloned()
            .collect())
    }

    async fn find_active_sessions_by_user(
        &self,
        user_id: u64,
        guild_id: u64,
    ) -> Result<Vec<VoiceSessionsEntity>, DatabaseError> {
        Ok(self
            .active
            .lock()
            .unwrap()
            .iter()
            .filter(|session| {
                session.is_active && session.user_id == user_id && session.guild_id == guild_id
            })
            .cloned()
            .collect())
    }

    async fn get_sessions_in_range(
        &self,
        _guild_id: u64,
        _user_id: Option<u64>,
        _since: &DateTime<Utc>,
        _until: &DateTime<Utc>,
    ) -> Result<Vec<VoiceSessionsEntity>, DatabaseError> {
        Ok(Vec::new())
    }

    async fn get_user_daily_activity(
        &self,
        _user_id: u64,
        _guild_id: u64,
        _since: &DateTime<Utc>,
        _until: &DateTime<Utc>,
    ) -> Result<Vec<VoiceDailyActivity>, DatabaseError> {
        Ok(Vec::new())
    }

    async fn get_guild_daily_stats(
        &self,
        _guild_id: u64,
        _since: &DateTime<Utc>,
        _until: &DateTime<Utc>,
        _stat_type: GuildStatType,
    ) -> Result<Vec<GuildDailyStats>, DatabaseError> {
        Ok(Vec::new())
    }
}

struct MemorySettings;

#[async_trait]
impl VoiceSettingsRepository for MemorySettings {
    async fn list_all(&self) -> Result<Vec<VoiceSettingsEntity>, DatabaseError> {
        Ok(Vec::new())
    }

    async fn get(&self, _guild_id: u64) -> Result<Option<VoiceSettingsEntity>, DatabaseError> {
        Ok(None)
    }

    async fn replace(&self, _settings: &VoiceSettingsEntity) -> Result<(), DatabaseError> {
        Ok(())
    }

    async fn delete_all(&self) -> Result<(), DatabaseError> {
        Ok(())
    }
}

async fn service(sessions: Arc<MemorySessions>) -> VoiceTrackingService {
    VoiceTrackingService::new(sessions, Arc::new(MemorySettings))
        .await
        .expect("construct service")
}

#[tokio::test]
async fn heartbeat_crash_recovery_no_heartbeat() {
    let directory = tempfile::tempdir().expect("temporary data path");
    let sessions = Arc::new(MemorySessions {
        active: Mutex::new(Vec::new()),
    });
    let manager = VoiceHeartbeatManager::new(directory.path(), Arc::new(service(sessions).await));

    assert_eq!(manager.read_last_heartbeat().await.unwrap(), None);
    assert_eq!(manager.recover_from_crash().await.unwrap(), 0);
}

#[tokio::test]
async fn heartbeat_read_write() {
    let directory = tempfile::tempdir().expect("temporary data path");
    let sessions = Arc::new(MemorySessions {
        active: Mutex::new(Vec::new()),
    });
    let manager = VoiceHeartbeatManager::new(directory.path(), Arc::new(service(sessions).await));

    manager.update().await;

    assert!(manager.read_last_heartbeat().await.unwrap().is_some());
    assert!(manager.path().starts_with(directory.path()));
}

#[tokio::test]
async fn heartbeat_crash_recovery_with_active_sessions() {
    let directory = tempfile::tempdir().expect("temporary data path");
    let now = Utc::now();
    let heartbeat_time = now - Duration::minutes(5);
    let sessions = Arc::new(MemorySessions {
        active: Mutex::new(vec![VoiceSessionsEntity {
            id: 1,
            user_id: 42,
            guild_id: 7,
            channel_id: 9,
            join_time: now - Duration::hours(1),
            leave_time: now - Duration::hours(1),
            is_active: true,
        }]),
    });
    let manager =
        VoiceHeartbeatManager::new(directory.path(), Arc::new(service(sessions.clone()).await));
    std::fs::write(manager.path(), heartbeat_time.to_rfc3339()).expect("seed heartbeat");

    assert_eq!(manager.recover_from_crash().await.unwrap(), 1);
    let active = sessions.active.lock().unwrap();
    assert!(!active[0].is_active);
    assert_eq!(active[0].leave_time, heartbeat_time);
}

#[tokio::test]
async fn heartbeat_crash_recovery_no_sessions() {
    let directory = tempfile::tempdir().expect("temporary data path");
    let sessions = Arc::new(MemorySessions {
        active: Mutex::new(Vec::new()),
    });
    let manager = VoiceHeartbeatManager::new(directory.path(), Arc::new(service(sessions).await));
    std::fs::write(
        manager.path(),
        (Utc::now() - Duration::minutes(5)).to_rfc3339(),
    )
    .expect("seed heartbeat");
    assert_eq!(manager.recover_from_crash().await.unwrap(), 0);
}

#[tokio::test]
async fn find_active_sessions() {
    let now = Utc::now();
    let sessions = Arc::new(MemorySessions {
        active: Mutex::new(vec![
            VoiceSessionsEntity {
                id: 1,
                user_id: 1001,
                guild_id: 555,
                channel_id: 9001,
                join_time: now - Duration::hours(2),
                leave_time: now - Duration::hours(2),
                is_active: true,
            },
            VoiceSessionsEntity {
                id: 2,
                user_id: 1002,
                guild_id: 555,
                channel_id: 9001,
                join_time: now - Duration::hours(3),
                leave_time: now - Duration::hours(1),
                is_active: false,
            },
            VoiceSessionsEntity {
                id: 3,
                user_id: 1003,
                guild_id: 555,
                channel_id: 9001,
                join_time: now - Duration::minutes(30),
                leave_time: now - Duration::minutes(30),
                is_active: true,
            },
        ]),
    });
    let service = service(sessions).await;
    let active = service.find_active_sessions().await.unwrap();
    assert_eq!(active.len(), 2);
    let user_ids: Vec<u64> = active.iter().map(|session| session.user_id).collect();
    assert!(user_ids.contains(&1001));
    assert!(user_ids.contains(&1003));
    assert!(!user_ids.contains(&1002));
}

#[tokio::test]
async fn update_session_leave_time() {
    let now = Utc::now();
    let join_time = now - Duration::hours(1);
    let sessions = Arc::new(MemorySessions {
        active: Mutex::new(vec![VoiceSessionsEntity {
            id: 1,
            user_id: 1001,
            guild_id: 555,
            channel_id: 9001,
            join_time,
            leave_time: join_time,
            is_active: true,
        }]),
    });
    let service = service(sessions.clone()).await;
    service
        .update_session_leave_time(1001, 9001, &join_time, &now)
        .await
        .expect("update leave time");
    let stored = &sessions.active.lock().unwrap()[0];
    assert!(!stored.is_active);
    assert_eq!(stored.leave_time, now);
    assert_ne!(stored.leave_time, stored.join_time);
}
