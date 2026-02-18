//! Integration tests for voice tracking service.

use chrono::Duration;
use chrono::Utc;
use pwr_bot::repository::model::ServerSettings;
use pwr_bot::repository::model::ServerSettingsModel;
use pwr_bot::repository::model::VoiceSessionsModel;
use pwr_bot::repository::model::VoiceSettings;
use pwr_bot::repository::table::Table;
use pwr_bot::service::voice_tracking_service::VoiceTrackingService;

mod common;

#[tokio::test]
async fn test_voice_tracking_service_new() {
    let (db, db_path) = common::setup_db().await;

    // Test creating the service
    let service = VoiceTrackingService::new(db.clone()).await;
    assert!(service.is_ok(), "Failed to create VoiceTrackingService");

    common::teardown_db(db_path).await;
}

#[tokio::test]
async fn test_is_enabled_default() {
    let (db, db_path) = common::setup_db().await;
    let service = VoiceTrackingService::new(db.clone())
        .await
        .expect("Failed to create service");

    let guild_id: u64 = 123456789;

    // By default, voice tracking should be enabled
    let is_enabled = service.is_enabled(guild_id).await;
    assert!(is_enabled, "Voice tracking should be enabled by default");

    common::teardown_db(db_path).await;
}

#[tokio::test]
async fn test_is_enabled_when_disabled() {
    let (db, db_path) = common::setup_db().await;
    let service = VoiceTrackingService::new(db.clone())
        .await
        .expect("Failed to create service");

    let guild_id: u64 = 123456789;

    // Disable voice tracking for the guild
    let voice_settings = VoiceSettings {
        enabled: Some(false),
    };
    let settings = ServerSettings {
        voice: voice_settings,
        ..Default::default()
    };
    service
        .update_server_settings(guild_id, settings)
        .await
        .expect("Failed to update settings");

    // Voice tracking should be disabled
    let is_enabled = service.is_enabled(guild_id).await;
    assert!(!is_enabled, "Voice tracking should be disabled");

    common::teardown_db(db_path).await;
}

#[tokio::test]
async fn test_is_enabled_when_re_enabled() {
    let (db, db_path) = common::setup_db().await;
    let service = VoiceTrackingService::new(db.clone())
        .await
        .expect("Failed to create service");

    let guild_id: u64 = 123456789;

    // Disable voice tracking
    let voice_settings = VoiceSettings {
        enabled: Some(false),
    };
    let settings = ServerSettings {
        voice: voice_settings,
        ..Default::default()
    };
    service
        .update_server_settings(guild_id, settings.clone())
        .await
        .expect("Failed to update settings");
    assert!(!service.is_enabled(guild_id).await);

    // Re-enable voice tracking
    let voice_settings = VoiceSettings {
        enabled: Some(true),
    };
    let settings = ServerSettings {
        voice: voice_settings,
        ..Default::default()
    };
    service
        .update_server_settings(guild_id, settings)
        .await
        .expect("Failed to update settings");

    let is_enabled = service.is_enabled(guild_id).await;
    assert!(is_enabled, "Voice tracking should be re-enabled");

    common::teardown_db(db_path).await;
}

#[tokio::test]
async fn test_insert_and_replace_voice_session() {
    let (db, db_path) = common::setup_db().await;
    let service = VoiceTrackingService::new(db.clone())
        .await
        .expect("Failed to create service");

    let now = Utc::now();
    let session = VoiceSessionsModel {
        id: 0,
        user_id: 111111,
        guild_id: 222222,
        channel_id: 333333,
        join_time: now,
        leave_time: now + Duration::hours(1),
    };

    // Insert the session
    service
        .insert(&session)
        .await
        .expect("Failed to insert voice session");

    // Verify it was inserted by querying the database directly
    let sessions: Vec<VoiceSessionsModel> = db
        .voice_sessions
        .select_all()
        .await
        .expect("Failed to select sessions");
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].user_id, 111111);
    assert_eq!(sessions[0].guild_id, 222222);

    // Test replace (update) the session
    let updated_session = VoiceSessionsModel {
        id: sessions[0].id,
        user_id: 111111,
        guild_id: 222222,
        channel_id: 333333,
        join_time: now,
        leave_time: now + Duration::hours(2), // Changed duration
    };
    service
        .replace(&updated_session)
        .await
        .expect("Failed to replace voice session");

    let sessions: Vec<VoiceSessionsModel> = db
        .voice_sessions
        .select_all()
        .await
        .expect("Failed to select sessions");
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].leave_time, now + Duration::hours(2));

    common::teardown_db(db_path).await;
}

#[tokio::test]
async fn test_get_server_settings_default() {
    let (db, db_path) = common::setup_db().await;
    let service = VoiceTrackingService::new(db.clone())
        .await
        .expect("Failed to create service");

    let guild_id: u64 = 123456789;

    // Get default settings for a guild that doesn't exist yet
    let settings = service
        .get_server_settings(guild_id)
        .await
        .expect("Failed to get settings");

    assert!(settings.voice.enabled.is_none());

    common::teardown_db(db_path).await;
}

#[tokio::test]
async fn test_update_and_get_server_settings() {
    let (db, db_path) = common::setup_db().await;
    let service = VoiceTrackingService::new(db.clone())
        .await
        .expect("Failed to create service");

    let guild_id: u64 = 123456789;

    // Update settings
    let voice_settings = VoiceSettings {
        enabled: Some(true),
    };
    let settings = ServerSettings {
        voice: voice_settings,
        ..Default::default()
    };
    service
        .update_server_settings(guild_id, settings.clone())
        .await
        .expect("Failed to update settings");

    // Get settings and verify
    let fetched = service
        .get_server_settings(guild_id)
        .await
        .expect("Failed to get settings");
    assert_eq!(fetched.voice.enabled, Some(true));

    common::teardown_db(db_path).await;
}

#[tokio::test]
async fn test_get_leaderboard() {
    let (db, db_path) = common::setup_db().await;
    let service = VoiceTrackingService::new(db.clone())
        .await
        .expect("Failed to create service");

    let guild_id: u64 = 555555;
    let now = Utc::now();

    // Insert multiple voice sessions for different users
    let sessions = vec![
        VoiceSessionsModel {
            id: 0,
            user_id: 1001,
            guild_id,
            channel_id: 9001,
            join_time: now,
            leave_time: now + Duration::hours(1), // 3600 seconds
        },
        VoiceSessionsModel {
            id: 0,
            user_id: 1001,
            guild_id,
            channel_id: 9001,
            join_time: now + Duration::hours(2),
            leave_time: now + Duration::hours(4), // 7200 seconds, total: 10800
        },
        VoiceSessionsModel {
            id: 0,
            user_id: 1002,
            guild_id,
            channel_id: 9001,
            join_time: now,
            leave_time: now + Duration::minutes(30), // 1800 seconds
        },
        VoiceSessionsModel {
            id: 0,
            user_id: 1003,
            guild_id,
            channel_id: 9001,
            join_time: now,
            leave_time: now + Duration::hours(2), // 7200 seconds
        },
    ];

    for session in sessions {
        service
            .insert(&session)
            .await
            .expect("Failed to insert session");
    }

    // Get leaderboard
    let leaderboard = service
        .get_leaderboard(guild_id, 10)
        .await
        .expect("Failed to get leaderboard");

    // Should have 3 unique users
    assert_eq!(leaderboard.len(), 3);

    // User 1001 should be first with 10800 seconds (3 hours total)
    assert_eq!(leaderboard[0].user_id, 1001);
    assert_eq!(leaderboard[0].total_duration, 10800);

    // User 1003 should be second with 7200 seconds (2 hours)
    assert_eq!(leaderboard[1].user_id, 1003);
    assert_eq!(leaderboard[1].total_duration, 7200);

    // User 1002 should be third with 1800 seconds (30 minutes)
    assert_eq!(leaderboard[2].user_id, 1002);
    assert_eq!(leaderboard[2].total_duration, 1800);

    common::teardown_db(db_path).await;
}

#[tokio::test]
async fn test_get_leaderboard_with_limit() {
    let (db, db_path) = common::setup_db().await;
    let service = VoiceTrackingService::new(db.clone())
        .await
        .expect("Failed to create service");

    let guild_id: u64 = 666666;
    let now = Utc::now();

    // Insert sessions for 5 users
    for i in 1..=5 {
        let session = VoiceSessionsModel {
            id: 0,
            user_id: 2000 + i as u64,
            guild_id,
            channel_id: 9001,
            join_time: now,
            leave_time: now + Duration::hours(i as i64), // Each user has different duration
        };
        service
            .insert(&session)
            .await
            .expect("Failed to insert session");
    }

    // Get leaderboard with limit of 3
    let leaderboard = service
        .get_leaderboard(guild_id, 3)
        .await
        .expect("Failed to get leaderboard");

    // Should only have 3 entries
    assert_eq!(leaderboard.len(), 3);

    // Top user should have 5 hours
    assert_eq!(leaderboard[0].user_id, 2005);
    assert_eq!(leaderboard[0].total_duration, 5 * 3600);

    common::teardown_db(db_path).await;
}

#[tokio::test]
async fn test_get_leaderboard_with_offset() {
    let (db, db_path) = common::setup_db().await;
    let service = VoiceTrackingService::new(db.clone())
        .await
        .expect("Failed to create service");

    let guild_id: u64 = 777777;
    let now = Utc::now();

    // Insert sessions for 5 users
    for i in 1..=5 {
        let session = VoiceSessionsModel {
            id: 0,
            user_id: 3000 + i as u64,
            guild_id,
            channel_id: 9001,
            join_time: now,
            leave_time: now + Duration::hours(i as i64),
        };
        service
            .insert(&session)
            .await
            .expect("Failed to insert session");
    }

    // Get leaderboard with offset 2 and limit 2
    let leaderboard = service
        .get_leaderboard_with_offset(guild_id, 2, 2)
        .await
        .expect("Failed to get leaderboard");

    // Should have 2 entries (positions 3 and 4)
    assert_eq!(leaderboard.len(), 2);

    // Third place user (3003) with 3 hours
    assert_eq!(leaderboard[0].user_id, 3003);
    assert_eq!(leaderboard[0].total_duration, 3 * 3600);

    // Fourth place user (3002) with 2 hours
    assert_eq!(leaderboard[1].user_id, 3002);
    assert_eq!(leaderboard[1].total_duration, 2 * 3600);

    common::teardown_db(db_path).await;
}

#[tokio::test]
async fn test_get_leaderboard_empty() {
    let (db, db_path) = common::setup_db().await;
    let service = VoiceTrackingService::new(db.clone())
        .await
        .expect("Failed to create service");

    let guild_id: u64 = 888888;

    // Get leaderboard for guild with no sessions
    let leaderboard = service
        .get_leaderboard(guild_id, 10)
        .await
        .expect("Failed to get leaderboard");

    // Should be empty
    assert!(leaderboard.is_empty());

    common::teardown_db(db_path).await;
}

#[tokio::test]
async fn test_disabled_guilds_cache_on_init() {
    let (db, db_path) = common::setup_db().await;

    // Pre-populate database with disabled guild
    let disabled_guild_id: u64 = 999999;
    let settings = ServerSettingsModel {
        guild_id: disabled_guild_id,
        settings: sqlx::types::Json(ServerSettings {
            voice: VoiceSettings {
                enabled: Some(false),
            },
            ..Default::default()
        }),
    };
    db.server_settings
        .replace(&settings)
        .await
        .expect("Failed to insert settings");

    // Create service - should load disabled guilds from database
    let service = VoiceTrackingService::new(db.clone())
        .await
        .expect("Failed to create service");

    // Check that the pre-populated disabled guild is in the cache
    let is_enabled = service.is_enabled(disabled_guild_id).await;
    assert!(!is_enabled, "Guild should be disabled from cache");

    // Check that a new guild is enabled by default
    let is_enabled_new = service.is_enabled(111111).await;
    assert!(is_enabled_new, "New guild should be enabled by default");

    common::teardown_db(db_path).await;
}

#[tokio::test]
async fn test_get_leaderboard_includes_active_sessions() {
    let (db, db_path) = common::setup_db().await;
    let service = VoiceTrackingService::new(db.clone())
        .await
        .expect("Failed to create service");

    let guild_id: u64 = 999999;
    let now = Utc::now();

    // Insert a mix of completed and active sessions
    // Active session: leave_time == join_time
    let sessions = vec![
        VoiceSessionsModel {
            id: 0,
            user_id: 2001,
            guild_id,
            channel_id: 9001,
            join_time: now - Duration::hours(2),
            leave_time: now, // Completed: 2 hours (7200 seconds)
        },
        VoiceSessionsModel {
            id: 0,
            user_id: 2002,
            guild_id,
            channel_id: 9001,
            join_time: now - Duration::hours(1),
            leave_time: now - Duration::hours(1), // Active: 1 hour so far
        },
        VoiceSessionsModel {
            id: 0,
            user_id: 2003,
            guild_id,
            channel_id: 9001,
            join_time: now - Duration::minutes(30),
            leave_time: now - Duration::minutes(30), // Active: 30 minutes so far
        },
    ];

    for session in &sessions {
        service
            .insert(session)
            .await
            .expect("Failed to insert session");
    }

    // Get leaderboard
    let leaderboard = service
        .get_leaderboard(guild_id, 10)
        .await
        .expect("Failed to get leaderboard");

    // Should have 3 users
    assert_eq!(leaderboard.len(), 3, "Should have 3 users in leaderboard");

    // User 2001 should be first with ~7200 seconds (2 hours completed)
    assert_eq!(leaderboard[0].user_id, 2001);
    assert!(
        leaderboard[0].total_duration >= 7200,
        "User 2001 should have at least 7200 seconds"
    );

    // User 2002 should be second with ~3600 seconds (1 hour active)
    assert_eq!(leaderboard[1].user_id, 2002);
    assert!(
        leaderboard[1].total_duration >= 3600,
        "User 2002 should have at least 3600 seconds"
    );

    // User 2003 should be third with ~1800 seconds (30 minutes active)
    assert_eq!(leaderboard[2].user_id, 2003);
    assert!(
        leaderboard[2].total_duration >= 1800,
        "User 2003 should have at least 1800 seconds"
    );

    common::teardown_db(db_path).await;
}

#[tokio::test]
async fn test_get_leaderboard_active_and_completed_mixed() {
    let (db, db_path) = common::setup_db().await;
    let service = VoiceTrackingService::new(db.clone())
        .await
        .expect("Failed to create service");

    let guild_id: u64 = 888888;
    let now = Utc::now();

    // User with multiple sessions: one completed, one active
    let sessions = vec![
        // User 3001: Completed session (1 hour)
        VoiceSessionsModel {
            id: 0,
            user_id: 3001,
            guild_id,
            channel_id: 9001,
            join_time: now - Duration::hours(3),
            leave_time: now - Duration::hours(2),
        },
        // User 3001: Active session (30 minutes so far)
        VoiceSessionsModel {
            id: 0,
            user_id: 3001,
            guild_id,
            channel_id: 9001,
            join_time: now - Duration::minutes(30),
            leave_time: now - Duration::minutes(30),
        },
        // User 3002: Only completed sessions (2 hours total)
        VoiceSessionsModel {
            id: 0,
            user_id: 3002,
            guild_id,
            channel_id: 9001,
            join_time: now - Duration::hours(4),
            leave_time: now - Duration::hours(3),
        },
        VoiceSessionsModel {
            id: 0,
            user_id: 3002,
            guild_id,
            channel_id: 9001,
            join_time: now - Duration::hours(2),
            leave_time: now - Duration::hours(1),
        },
    ];

    for session in &sessions {
        service
            .insert(session)
            .await
            .expect("Failed to insert session");
    }

    // Get leaderboard
    let leaderboard = service
        .get_leaderboard(guild_id, 10)
        .await
        .expect("Failed to get leaderboard");

    assert_eq!(leaderboard.len(), 2, "Should have 2 users");

    // User 3002 should be first with exactly 7200 seconds (2 hours completed)
    assert_eq!(leaderboard[0].user_id, 3002);
    assert_eq!(
        leaderboard[0].total_duration, 7200,
        "User 3002 should have exactly 7200 seconds"
    );

    // User 3001 should be second with ~5400 seconds (3600 completed + ~1800 active)
    assert_eq!(leaderboard[1].user_id, 3001);
    assert!(
        leaderboard[1].total_duration >= 5400,
        "User 3001 should have at least 5400 seconds (3600 completed + ~1800 active)"
    );

    common::teardown_db(db_path).await;
}
