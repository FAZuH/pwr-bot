//! Integration tests for voice tracking heartbeat mechanism.

use std::sync::Arc;

use chrono::Duration;
use chrono::Utc;
use pwr_bot::repository::model::VoiceSessionsModel;
use pwr_bot::repository::table::Table;
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

    // Create temp directory for heartbeat file
    let temp_dir = std::env::temp_dir();
    let heartbeat_manager = VoiceHeartbeatManager::new(&temp_dir, service);

    // Initially there should be no heartbeat file
    let last_heartbeat = heartbeat_manager
        .read_last_heartbeat()
        .await
        .expect("Failed to read heartbeat");
    assert!(last_heartbeat.is_none(), "Should be no heartbeat initially");

    // Cleanup
    let heartbeat_path = temp_dir.join("voice_heartbeat.json");
    if heartbeat_path.exists() {
        let _ = std::fs::remove_file(&heartbeat_path);
    }

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

    // Create a unique temp directory for this test
    let temp_dir =
        std::env::temp_dir().join(format!("pwr-bot-test-no-sessions-{}", std::process::id()));
    std::fs::create_dir_all(&temp_dir).expect("Failed to create temp directory");
    let heartbeat_manager = VoiceHeartbeatManager::new(&temp_dir, service.clone());

    // Write a heartbeat timestamp
    let heartbeat_time = Utc::now() - Duration::minutes(5);
    let heartbeat_data = serde_json::json!({
        "timestamp": heartbeat_time,
        "version": 1
    });
    let heartbeat_path = temp_dir.join("voice_heartbeat.json");
    std::fs::write(&heartbeat_path, heartbeat_data.to_string())
        .expect("Failed to write heartbeat file");

    // Recover from crash - should close 0 sessions since there are none
    let recovered = heartbeat_manager
        .recover_from_crash()
        .await
        .expect("Failed to recover");
    assert_eq!(recovered, 0, "Should recover 0 sessions when none exist");

    // Cleanup
    if temp_dir.exists() {
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

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

    // Create a unique temp directory for this test
    let temp_dir = std::env::temp_dir().join(format!("pwr-bot-test-active-{}", std::process::id()));
    std::fs::create_dir_all(&temp_dir).expect("Failed to create temp directory");
    let heartbeat_manager = VoiceHeartbeatManager::new(&temp_dir, service.clone());

    // Create active sessions (leave_time == join_time)
    let now = Utc::now();
    let active_sessions = vec![
        VoiceSessionsModel {
            id: 0,
            user_id: 1001,
            guild_id: 555555,
            channel_id: 9001,
            join_time: now - Duration::hours(2),
            leave_time: now - Duration::hours(2), // Active session
        },
        VoiceSessionsModel {
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

    // Write a heartbeat timestamp from 5 minutes ago
    let heartbeat_time = now - Duration::minutes(5);
    let heartbeat_data = serde_json::json!({
        "timestamp": heartbeat_time,
        "version": 1
    });
    let heartbeat_path = temp_dir.join("voice_heartbeat.json");
    std::fs::write(&heartbeat_path, heartbeat_data.to_string())
        .expect("Failed to write heartbeat file");

    // Recover from crash
    let recovered = heartbeat_manager
        .recover_from_crash()
        .await
        .expect("Failed to recover");
    assert_eq!(recovered, 2, "Should recover 2 active sessions");

    // Verify sessions were closed with heartbeat timestamp
    let sessions: Vec<VoiceSessionsModel> = db
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

    // Cleanup
    if temp_dir.exists() {
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    common::teardown_db(db_path).await;
}

#[tokio::test]
async fn test_heartbeat_crash_recovery_no_heartbeat_file() {
    let (db, db_path) = common::setup_db().await;
    let service = Arc::new(
        VoiceTrackingService::new(db.clone())
            .await
            .expect("Failed to create service"),
    );

    // Create a unique temp directory for this test
    let temp_dir = std::env::temp_dir().join(format!("pwr-bot-test-no-hb-{}", std::process::id()));
    std::fs::create_dir_all(&temp_dir).expect("Failed to create temp directory");
    let heartbeat_path = temp_dir.join("voice_heartbeat.json");

    // Make sure heartbeat file doesn't exist
    if heartbeat_path.exists() {
        let _ = std::fs::remove_file(&heartbeat_path);
    }

    let heartbeat_manager = VoiceHeartbeatManager::new(&temp_dir, service.clone());

    // Create active sessions
    let now = Utc::now();
    let session = VoiceSessionsModel {
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

    // Recover from crash without heartbeat file
    let recovered = heartbeat_manager
        .recover_from_crash()
        .await
        .expect("Failed to recover");
    assert_eq!(
        recovered, 0,
        "Should not recover sessions without heartbeat file"
    );

    // Verify session is still active (not closed)
    let sessions: Vec<VoiceSessionsModel> = db
        .voice_sessions
        .select_all()
        .await
        .expect("Failed to select sessions");

    assert_eq!(sessions.len(), 1);
    assert_eq!(
        sessions[0].leave_time, sessions[0].join_time,
        "Session should still be active"
    );

    // Cleanup temp directory
    if temp_dir.exists() {
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

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
        VoiceSessionsModel {
            id: 0,
            user_id: 1001,
            guild_id: 555555,
            channel_id: 9001,
            join_time: now - Duration::hours(2),
            leave_time: now - Duration::hours(2), // Active
        },
        VoiceSessionsModel {
            id: 0,
            user_id: 1002,
            guild_id: 555555,
            channel_id: 9001,
            join_time: now - Duration::hours(3),
            leave_time: now - Duration::hours(1), // Completed (2 hours)
        },
        VoiceSessionsModel {
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
    let session = VoiceSessionsModel {
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
    let sessions: Vec<VoiceSessionsModel> = db
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
    let sessions: Vec<VoiceSessionsModel> = db
        .voice_sessions
        .select_all()
        .await
        .expect("Failed to select sessions");

    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].leave_time, new_leave_time);
    assert_ne!(sessions[0].leave_time, sessions[0].join_time);

    common::teardown_db(db_path).await;
}
