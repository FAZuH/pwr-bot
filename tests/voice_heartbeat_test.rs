//! Integration tests for voice tracking heartbeat mechanism.

use std::sync::Arc;

use chrono::Duration;
use chrono::Utc;
use pwr_bot::entity::BotMetaKey;
use pwr_bot::entity::VoiceSessionsEntity;
use pwr_bot::repository::table::Table;
use pwr_bot::service::internal_service::InternalService;
use pwr_bot::service::voice_tracking_service::VoiceTrackingService;
use pwr_bot::task::voice_heartbeat::VoiceHeartbeatManager;

mod common;

#[tokio::test]
async fn test_heartbeat_read_write() {
    let (db, db_path) = common::setup_db().await;
    let service = Arc::new(
        VoiceTrackingService::new(db.clone())
            .await
            .expect("Failed to create service"),
    );
    let internal = Arc::new(InternalService::new(db.clone()));
    let heartbeat_manager = VoiceHeartbeatManager::new(internal, service);

    // Initially there should be no heartbeat
    let last_heartbeat = heartbeat_manager
        .read_last_heartbeat()
        .await
        .expect("Failed to read heartbeat");
    assert!(last_heartbeat.is_none(), "Should be no heartbeat initially");

    common::teardown_db(db_path).await;
}

#[tokio::test]
async fn test_heartbeat_crash_recovery_no_sessions() {
    let (db, db_path) = common::setup_db().await;
    let service = Arc::new(
        VoiceTrackingService::new(db.clone())
            .await
            .expect("Failed to create service"),
    );
    let internal = Arc::new(InternalService::new(db.clone()));

    // Write a heartbeat timestamp directly to database
    let heartbeat_time = Utc::now() - Duration::minutes(5);
    internal
        .set_meta(BotMetaKey::VoiceHeartbeat, &heartbeat_time.to_rfc3339())
        .await
        .expect("Failed to set heartbeat");

    let heartbeat_manager = VoiceHeartbeatManager::new(internal.clone(), service.clone());

    // Recover from crash - should close 0 sessions since there are none
    let recovered = heartbeat_manager
        .recover_from_crash()
        .await
        .expect("Failed to recover");
    assert_eq!(recovered, 0, "Should recover 0 sessions when none exist");

    common::teardown_db(db_path).await;
}

#[tokio::test]
async fn test_heartbeat_crash_recovery_with_active_sessions() {
    let (db, db_path) = common::setup_db().await;
    let service = Arc::new(
        VoiceTrackingService::new(db.clone())
            .await
            .expect("Failed to create service"),
    );
    let internal = Arc::new(InternalService::new(db.clone()));

    // Create active sessions (leave_time == join_time)
    let now = Utc::now();
    let active_sessions = vec![
        VoiceSessionsEntity {
            id: 0,
            user_id: 1001,
            guild_id: 555555,
            channel_id: 9001,
            join_time: now - Duration::hours(2),
            leave_time: now - Duration::hours(2), // Active session
        },
        VoiceSessionsEntity {
            id: 0,
            user_id: 1002,
            guild_id: 555555,
            channel_id: 9001,
            join_time: now - Duration::minutes(30),
            leave_time: now - Duration::minutes(30), // Active session
        },
    ];

    for session in &active_sessions {
        service
            .insert(session)
            .await
            .expect("Failed to insert session");
    }

    // Write a heartbeat timestamp from 5 minutes ago directly to database
    let heartbeat_time = now - Duration::minutes(5);
    internal
        .set_meta(BotMetaKey::VoiceHeartbeat, &heartbeat_time.to_rfc3339())
        .await
        .expect("Failed to set heartbeat");

    let heartbeat_manager = VoiceHeartbeatManager::new(internal.clone(), service.clone());

    // Recover from crash
    let recovered = heartbeat_manager
        .recover_from_crash()
        .await
        .expect("Failed to recover");
    assert_eq!(recovered, 2, "Should recover 2 active sessions");

    // Verify sessions were closed with heartbeat timestamp
    let sessions: Vec<VoiceSessionsEntity> = db
        .voice_sessions
        .select_all()
        .await
        .expect("Failed to select sessions");

    assert_eq!(sessions.len(), 2);
    for session in sessions {
        // Both sessions should now have leave_time set to heartbeat_time (not equal to join_time)
        assert_ne!(
            session.leave_time, session.join_time,
            "Session should be closed after recovery"
        );
        // The leave_time should be approximately the heartbeat time
        let diff = (session.leave_time - heartbeat_time).num_seconds().abs();
        assert!(
            diff < 2,
            "Leave time should be close to heartbeat time, diff: {}s",
            diff
        );
    }

    common::teardown_db(db_path).await;
}

#[tokio::test]
async fn test_heartbeat_crash_recovery_no_heartbeat() {
    let (db, db_path) = common::setup_db().await;
    let service = Arc::new(
        VoiceTrackingService::new(db.clone())
            .await
            .expect("Failed to create service"),
    );
    let internal = Arc::new(InternalService::new(db.clone()));

    let heartbeat_manager = VoiceHeartbeatManager::new(internal.clone(), service.clone());

    // Create active sessions
    let now = Utc::now();
    let session = VoiceSessionsEntity {
        id: 0,
        user_id: 1001,
        guild_id: 555555,
        channel_id: 9001,
        join_time: now - Duration::hours(1),
        leave_time: now - Duration::hours(1),
    };

    service
        .insert(&session)
        .await
        .expect("Failed to insert session");

    // Recover from crash without heartbeat
    let recovered = heartbeat_manager
        .recover_from_crash()
        .await
        .expect("Failed to recover");
    assert_eq!(
        recovered, 0,
        "Should not recover sessions without heartbeat"
    );

    // Verify session is still active (not closed)
    let sessions: Vec<VoiceSessionsEntity> = db
        .voice_sessions
        .select_all()
        .await
        .expect("Failed to select sessions");

    assert_eq!(sessions.len(), 1);
    assert_eq!(
        sessions[0].leave_time, sessions[0].join_time,
        "Session should still be active"
    );

    common::teardown_db(db_path).await;
}

#[tokio::test]
async fn test_find_active_sessions() {
    let (db, db_path) = common::setup_db().await;
    let service = VoiceTrackingService::new(db.clone())
        .await
        .expect("Failed to create service");

    let now = Utc::now();

    // Insert a mix of active and completed sessions
    let sessions = vec![
        VoiceSessionsEntity {
            id: 0,
            user_id: 1001,
            guild_id: 555555,
            channel_id: 9001,
            join_time: now - Duration::hours(2),
            leave_time: now - Duration::hours(2), // Active
        },
        VoiceSessionsEntity {
            id: 0,
            user_id: 1002,
            guild_id: 555555,
            channel_id: 9001,
            join_time: now - Duration::hours(3),
            leave_time: now - Duration::hours(1), // Completed (2 hours)
        },
        VoiceSessionsEntity {
            id: 0,
            user_id: 1003,
            guild_id: 555555,
            channel_id: 9001,
            join_time: now - Duration::minutes(30),
            leave_time: now - Duration::minutes(30), // Active
        },
    ];

    for session in &sessions {
        service
            .insert(session)
            .await
            .expect("Failed to insert session");
    }

    // Find active sessions
    let active = service
        .find_active_sessions()
        .await
        .expect("Failed to find active sessions");

    assert_eq!(active.len(), 2, "Should find 2 active sessions");

    // Verify correct users are found
    let user_ids: Vec<u64> = active.iter().map(|s| s.user_id).collect();
    assert!(user_ids.contains(&1001), "User 1001 should be active");
    assert!(user_ids.contains(&1003), "User 1003 should be active");
    assert!(!user_ids.contains(&1002), "User 1002 should not be active");

    common::teardown_db(db_path).await;
}

#[tokio::test]
async fn test_update_session_leave_time() {
    let (db, db_path) = common::setup_db().await;
    let service = VoiceTrackingService::new(db.clone())
        .await
        .expect("Failed to create service");

    let now = Utc::now();
    let join_time = now - Duration::hours(1);

    // Insert an active session
    let session = VoiceSessionsEntity {
        id: 0,
        user_id: 1001,
        guild_id: 555555,
        channel_id: 9001,
        join_time,
        leave_time: join_time, // Active
    };

    service
        .insert(&session)
        .await
        .expect("Failed to insert session");

    // Verify it's active
    let sessions: Vec<VoiceSessionsEntity> = db
        .voice_sessions
        .select_all()
        .await
        .expect("Failed to select sessions");
    assert_eq!(sessions[0].leave_time, sessions[0].join_time);

    // Update leave_time
    let new_leave_time = now;
    service
        .update_session_leave_time(1001, 9001, &join_time, &new_leave_time)
        .await
        .expect("Failed to update leave time");

    // Verify it was updated
    let sessions: Vec<VoiceSessionsEntity> = db
        .voice_sessions
        .select_all()
        .await
        .expect("Failed to select sessions");

    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].leave_time, new_leave_time);
    assert_ne!(sessions[0].leave_time, sessions[0].join_time);

    common::teardown_db(db_path).await;
}
