use std::sync::Arc;

use chrono::Duration;
use chrono::SubsecRound;
use chrono::Utc;
use serde_json::json;
use voice::VoiceLeaderboardOpt;
use voice::VoiceLeaderboardOptBuilder;
use voice::VoiceSessionsEntity;
use voice::VoiceSettingsEntity;
use voice::repo::Repository;
use voice::repo::traits::VoiceSessionsRepository;
use voice::repo::traits::VoiceSettingsRepository;
use voice::service::VoiceTrackingService;
use voice::subscriber::VoiceState;
use voice::subscriber::VoiceStateEvent;
use voice::subscriber::VoiceStateSubscriber;

#[path = "support/db.rs"]
mod db;

async fn setup_repository() -> Repository {
    let db_url = db::db_url().await;
    let repository = Repository::connect(&db_url)
        .await
        .expect("connect voice storage");
    repository.migrate().await.expect("run voice migrations");
    repository
        .delete_all()
        .await
        .expect("start with empty voice tables");
    repository
}

async fn make_service(repository: &Repository) -> VoiceTrackingService {
    VoiceTrackingService::new(
        Arc::new(repository.voice_sessions.clone()),
        Arc::new(repository.voice_settings.clone()),
    )
    .await
    .expect("construct voice service")
}

fn leaderboard_options(guild_id: u64, offset: u32, limit: u32) -> VoiceLeaderboardOpt {
    VoiceLeaderboardOptBuilder::default()
        .guild_id(guild_id)
        .offset(Some(offset))
        .limit(Some(limit))
        .since(Some(chrono::DateTime::UNIX_EPOCH))
        .until(Some(Utc::now() + Duration::days(365)))
        .build()
        .expect("leaderboard options")
}

macro_rules! voice_repository_test {
    ($name:ident, |$repo:ident| $body:block) => {
        #[tokio::test]
        #[serial_test::serial]
        async fn $name() {
            let $repo = setup_repository().await;
            $body
            $repo.delete_all().await.expect("clean voice tables");
        }
    };
}

voice_repository_test!(voice_sessions_update, |repo| {
    let id = repo
        .voice_sessions
        .insert(&VoiceSessionsEntity {
            user_id: 1,
            guild_id: 2,
            channel_id: 3,
            join_time: Utc::now(),
            leave_time: Utc::now() + Duration::hours(1),
            is_active: false,
            ..Default::default()
        })
        .await
        .expect("insert session");
    let mut session = repo
        .voice_sessions
        .select_all()
        .await
        .expect("select sessions")
        .into_iter()
        .find(|session| session.id == id)
        .expect("inserted session");
    session.channel_id = 4;
    repo.voice_sessions
        .replace(&session)
        .await
        .expect("replace session");
    let updated = repo
        .voice_sessions
        .select_all()
        .await
        .expect("select updated session")
        .into_iter()
        .find(|session| session.id == id)
        .expect("updated session");
    assert_eq!(updated.channel_id, 4);
});

voice_repository_test!(voice_sessions_delete, |repo| {
    repo.voice_sessions
        .insert(&VoiceSessionsEntity {
            user_id: 1,
            guild_id: 2,
            channel_id: 3,
            join_time: Utc::now(),
            leave_time: Utc::now(),
            is_active: false,
            ..Default::default()
        })
        .await
        .expect("insert session");
    repo.voice_sessions
        .delete_all()
        .await
        .expect("delete sessions");
    assert!(repo.voice_sessions.select_all().await.unwrap().is_empty());
});

voice_repository_test!(voice_sessions_insert_and_select, |repo| {
    let id = repo
        .voice_sessions
        .insert(&VoiceSessionsEntity {
            user_id: 1,
            guild_id: 2,
            channel_id: 3,
            join_time: Utc::now(),
            leave_time: Utc::now() + Duration::hours(1),
            is_active: false,
            ..Default::default()
        })
        .await
        .expect("insert session");
    let session = repo
        .voice_sessions
        .select_all()
        .await
        .expect("select sessions")
        .into_iter()
        .find(|session| session.id == id)
        .expect("inserted session");
    assert_eq!(session.user_id, 1);
    assert_eq!(session.guild_id, 2);
    assert_eq!(session.channel_id, 3);
});

voice_repository_test!(voice_sessions_update_leave_time, |repo| {
    let join_time = Utc::now().trunc_subsecs(6);
    repo.voice_sessions
        .insert(&VoiceSessionsEntity {
            id: 0,
            user_id: 100,
            guild_id: 200,
            channel_id: 300,
            join_time,
            leave_time: join_time,
            is_active: true,
        })
        .await
        .expect("insert active session");
    let leave_time = join_time + Duration::hours(1);
    repo.voice_sessions
        .update_leave_time(100, 300, &join_time, &leave_time)
        .await
        .expect("update leave time");
    let sessions = repo.voice_sessions.select_all().await.unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].leave_time, leave_time);
    assert_ne!(sessions[0].leave_time, sessions[0].join_time);
});

voice_repository_test!(voice_sessions_find_active_sessions, |repo| {
    let now = Utc::now();
    for (user_id, channel_id, is_active) in [(100, 300, true), (101, 301, false), (102, 302, true)]
    {
        repo.voice_sessions
            .insert(&VoiceSessionsEntity {
                id: 0,
                user_id,
                guild_id: 200,
                channel_id,
                join_time: now - Duration::hours(1),
                leave_time: now - Duration::hours(1),
                is_active,
            })
            .await
            .expect("insert session");
    }
    let active = repo
        .voice_sessions
        .find_active_sessions()
        .await
        .expect("find active sessions");
    assert_eq!(active.len(), 2);
    let user_ids: Vec<u64> = active.iter().map(|session| session.user_id).collect();
    assert!(user_ids.contains(&100));
    assert!(user_ids.contains(&102));
    assert!(!user_ids.contains(&101));
});

voice_repository_test!(voice_sessions_find_active_empty, |repo| {
    let active = repo
        .voice_sessions
        .find_active_sessions()
        .await
        .expect("find active sessions");
    assert!(active.is_empty());
});

voice_repository_test!(voice_sessions_update_leave_time_no_match, |repo| {
    let join_time = Utc::now();
    let result = repo
        .voice_sessions
        .update_leave_time(999, 999, &join_time, &(join_time + Duration::hours(1)))
        .await;
    assert!(result.is_ok());
    assert!(repo.voice_sessions.select_all().await.unwrap().is_empty());
});

voice_repository_test!(voice_sessions_get_user_daily_activity, |repo| {
    let now = Utc::now();
    let today = now.date_naive();
    let yesterday = today - Duration::days(1);
    for (user_id, channel_id, join_time, leave_time) in [
        (100, 300, now, now + Duration::hours(2)),
        (
            100,
            301,
            now - Duration::days(1),
            now - Duration::days(1) + Duration::hours(1),
        ),
        (101, 302, now, now + Duration::minutes(30)),
    ] {
        repo.voice_sessions
            .insert(&VoiceSessionsEntity {
                user_id,
                guild_id: 200,
                channel_id,
                join_time,
                leave_time,
                is_active: false,
                ..Default::default()
            })
            .await
            .expect("insert daily session");
    }

    let activity = repo
        .voice_sessions
        .get_user_daily_activity(
            100,
            200,
            &(now - Duration::days(2)),
            &(now + Duration::days(1)),
        )
        .await
        .expect("get user daily activity");
    assert_eq!(activity.len(), 2);
    assert_eq!(
        activity
            .iter()
            .find(|activity| activity.day == today)
            .expect("today activity")
            .total_seconds,
        7200
    );
    assert_eq!(
        activity
            .iter()
            .find(|activity| activity.day == yesterday)
            .expect("yesterday activity")
            .total_seconds,
        3600
    );
});

voice_repository_test!(voice_sessions_get_user_daily_activity_empty, |repo| {
    let now = Utc::now();
    let activity = repo
        .voice_sessions
        .get_user_daily_activity(999, 200, &(now - Duration::days(7)), &now)
        .await
        .expect("get empty user activity");
    assert!(activity.is_empty());
});

voice_repository_test!(voice_sessions_get_guild_daily_average_time, |repo| {
    let now = Utc::now();
    for (user_id, guild_id, channel_id, duration) in [
        (100, 200, 300, Duration::hours(2)),
        (101, 200, 301, Duration::hours(1)),
        (102, 999, 302, Duration::minutes(30)),
    ] {
        repo.voice_sessions
            .insert(&VoiceSessionsEntity {
                user_id,
                guild_id,
                channel_id,
                join_time: now,
                leave_time: now + duration,
                is_active: false,
                ..Default::default()
            })
            .await
            .expect("insert average session");
    }
    let stats = repo
        .voice_sessions
        .get_guild_daily_stats(
            200,
            &(now - Duration::days(1)),
            &(now + Duration::days(1)),
            voice::GuildStatType::AverageTime,
        )
        .await
        .expect("get guild average stats");
    assert_eq!(stats.len(), 1);
    assert_eq!(stats[0].value, 5400);
});

voice_repository_test!(voice_sessions_get_guild_daily_user_count, |repo| {
    let now = Utc::now()
        .date_naive()
        .and_hms_opt(12, 0, 0)
        .unwrap()
        .and_utc();
    for (user_id, channel_id, join_time, leave_time) in [
        (100, 300, now, now + Duration::hours(1)),
        (101, 301, now, now + Duration::hours(2)),
        (
            100,
            302,
            now + Duration::minutes(30),
            now + Duration::minutes(90),
        ),
    ] {
        repo.voice_sessions
            .insert(&VoiceSessionsEntity {
                user_id,
                guild_id: 200,
                channel_id,
                join_time,
                leave_time,
                is_active: false,
                ..Default::default()
            })
            .await
            .expect("insert user-count session");
    }
    let stats = repo
        .voice_sessions
        .get_guild_daily_stats(
            200,
            &(now - Duration::days(1)),
            &(now + Duration::days(1)),
            voice::GuildStatType::ActiveUserCount,
        )
        .await
        .expect("get guild user-count stats");
    assert_eq!(stats.len(), 1);
    assert_eq!(stats[0].day, now.date_naive());
    assert_eq!(stats[0].value, 2);
});

voice_repository_test!(voice_sessions_get_guild_daily_stats_empty, |repo| {
    let now = Utc::now();
    for stat_type in [
        voice::GuildStatType::AverageTime,
        voice::GuildStatType::ActiveUserCount,
    ] {
        let stats = repo
            .voice_sessions
            .get_guild_daily_stats(999, &(now - Duration::days(7)), &now, stat_type)
            .await
            .expect("get empty guild stats");
        assert!(stats.is_empty());
    }
});

#[tokio::test]
#[serial_test::serial]
async fn voice_repository_persists_sessions_settings_and_queries() {
    let db_url = db::db_url().await;
    let repository = Repository::connect(&db_url)
        .await
        .expect("connect voice storage");
    let _ = repository.migrate().await.expect("run voice migrations");
    repository
        .delete_all()
        .await
        .expect("start with empty tables");

    let now = Utc::now();
    let session = VoiceSessionsEntity {
        id: 0,
        user_id: 42,
        guild_id: 7,
        channel_id: 9,
        join_time: now - Duration::minutes(10),
        leave_time: now,
        is_active: false,
    };
    let id = repository
        .voice_sessions
        .insert(&session)
        .await
        .expect("insert voice session");
    let stored = repository
        .voice_sessions
        .select_all()
        .await
        .expect("read voice sessions");
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].id, id);
    assert_eq!(stored[0].user_id, 42);

    repository
        .voice_settings
        .replace(&VoiceSettingsEntity {
            guild_id: 7.into(),
            enabled: false,
        })
        .await
        .expect("replace voice settings");
    assert!(
        !repository
            .voice_settings
            .get(7)
            .await
            .expect("read voice settings")
            .expect("settings row")
            .enabled
    );

    let options = VoiceLeaderboardOptBuilder::default()
        .guild_id(7)
        .since(Some(now - Duration::hours(1)))
        .until(Some(now))
        .build()
        .expect("leaderboard options");
    let leaderboard = repository
        .voice_sessions
        .get_leaderboard_opt(&options)
        .await
        .expect("query leaderboard");
    assert_eq!(leaderboard.len(), 1);
    assert_eq!(leaderboard[0].user_id, 42);
    assert!(leaderboard[0].total_duration >= 590);
}

#[tokio::test]
#[serial_test::serial]
async fn voice_repository_imports_legacy_settings_once() {
    let db_url = db::db_url().await;
    let repository = Repository::connect(&db_url)
        .await
        .expect("connect voice storage");
    repository.migrate().await.expect("run voice migrations");
    repository
        .delete_all()
        .await
        .expect("start with empty tables");

    let (client, connection) = tokio_postgres::connect(&db_url, tokio_postgres::NoTls)
        .await
        .expect("connect query client");
    tokio::spawn(async move {
        connection.await.expect("query connection");
    });
    client
        .batch_execute(
            "CREATE TABLE IF NOT EXISTS server_settings (guild_id BIGINT PRIMARY KEY, settings JSONB NOT NULL);\
             DELETE FROM voice_settings WHERE guild_id = 7;\
             DELETE FROM voice_settings_import_state;\
             INSERT INTO server_settings (guild_id, settings) VALUES (7, '{\"voice\":{\"enabled\":false}}')\
             ON CONFLICT (guild_id) DO UPDATE SET settings = EXCLUDED.settings;",
        )
        .await
        .expect("seed legacy settings");

    let raw = client
        .query_one(
            "SELECT (settings #>> '{voice,enabled}')::boolean FROM server_settings WHERE guild_id = 7",
            &[],
        )
        .await
        .unwrap();
    let raw: Option<bool> = raw.get(0);
    assert_eq!(raw, Some(false));
    assert_eq!(repository.import_legacy_settings_once().await.unwrap(), 1);
    assert_eq!(repository.import_legacy_settings_once().await.unwrap(), 0);
    assert!(
        !repository
            .voice_settings
            .get(7)
            .await
            .unwrap()
            .unwrap()
            .enabled
    );
}

voice_repository_test!(voice_tracking_service_new, |repo| {
    let _service = make_service(&repo).await;
});

voice_repository_test!(is_enabled_default, |repo| {
    let service = make_service(&repo).await;
    assert!(service.is_enabled(123_456_789).await);
});

voice_repository_test!(is_enabled_when_disabled, |repo| {
    let service = make_service(&repo).await;
    service
        .update_settings(123_456_789, voice::VoiceSettings { enabled: false })
        .await
        .expect("disable voice tracking");
    assert!(!service.is_enabled(123_456_789).await);
});

voice_repository_test!(is_enabled_when_re_enabled, |repo| {
    let service = make_service(&repo).await;
    service
        .update_settings(123_456_789, voice::VoiceSettings { enabled: false })
        .await
        .expect("disable voice tracking");
    assert!(!service.is_enabled(123_456_789).await);
    service
        .update_settings(123_456_789, voice::VoiceSettings { enabled: true })
        .await
        .expect("re-enable voice tracking");
    assert!(service.is_enabled(123_456_789).await);
});

voice_repository_test!(insert_and_replace_voice_session, |repo| {
    let service = make_service(&repo).await;
    let now = Utc::now().trunc_subsecs(6);
    let session = VoiceSessionsEntity {
        id: 0,
        user_id: 111_111,
        guild_id: 222_222,
        channel_id: 333_333,
        join_time: now,
        leave_time: now + Duration::hours(1),
        is_active: false,
    };
    service.insert(&session).await.expect("insert session");
    let stored = repo.voice_sessions.select_all().await.unwrap();
    assert_eq!(stored.len(), 1);
    let updated = VoiceSessionsEntity {
        id: stored[0].id,
        user_id: session.user_id,
        guild_id: session.guild_id,
        channel_id: session.channel_id,
        join_time: now,
        leave_time: now + Duration::hours(2),
        is_active: false,
    };
    service.replace(&updated).await.expect("replace session");
    let sessions = repo.voice_sessions.select_all().await.unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].leave_time, now + Duration::hours(2));
});

voice_repository_test!(get_server_settings_default, |repo| {
    let service = make_service(&repo).await;
    assert!(service.get_settings(123_456_789).await.unwrap().enabled);
});

voice_repository_test!(update_and_get_server_settings, |repo| {
    let service = make_service(&repo).await;
    service
        .update_settings(123_456_789, voice::VoiceSettings { enabled: false })
        .await
        .expect("update settings");
    assert!(!service.get_settings(123_456_789).await.unwrap().enabled);
});

voice_repository_test!(get_leaderboard, |repo| {
    let service = make_service(&repo).await;
    let now = Utc::now();
    let sessions = [
        (1001, now, now + Duration::hours(1)),
        (1001, now + Duration::hours(2), now + Duration::hours(4)),
        (1002, now, now + Duration::minutes(30)),
        (1003, now, now + Duration::hours(2)),
    ];
    for (user_id, join_time, leave_time) in sessions {
        service
            .insert(&VoiceSessionsEntity {
                id: 0,
                user_id,
                guild_id: 555_555,
                channel_id: 9001,
                join_time,
                leave_time,
                is_active: false,
            })
            .await
            .expect("insert leaderboard session");
    }
    let leaderboard = service
        .get_leaderboard_withopt(&leaderboard_options(555_555, 0, 10))
        .await
        .expect("get leaderboard");
    assert_eq!(leaderboard.len(), 3);
    assert_eq!(leaderboard[0].user_id, 1001);
    assert_eq!(leaderboard[0].total_duration, 10_800);
    assert_eq!(leaderboard[1].user_id, 1003);
    assert_eq!(leaderboard[1].total_duration, 7_200);
    assert_eq!(leaderboard[2].user_id, 1002);
    assert_eq!(leaderboard[2].total_duration, 1_800);
});

voice_repository_test!(get_leaderboard_with_limit, |repo| {
    let service = make_service(&repo).await;
    let now = Utc::now();
    for index in 1..=5 {
        service
            .insert(&VoiceSessionsEntity {
                id: 0,
                user_id: 2000 + index as u64,
                guild_id: 666_666,
                channel_id: 9001,
                join_time: now,
                leave_time: now + Duration::hours(i64::from(index)),
                is_active: false,
            })
            .await
            .expect("insert limited session");
    }
    let leaderboard = service
        .get_leaderboard_withopt(&leaderboard_options(666_666, 0, 3))
        .await
        .expect("get limited leaderboard");
    assert_eq!(leaderboard.len(), 3);
    assert_eq!(leaderboard[0].user_id, 2005);
    assert_eq!(leaderboard[0].total_duration, 5 * 3600);
});

voice_repository_test!(get_leaderboard_with_offset, |repo| {
    let service = make_service(&repo).await;
    let now = Utc::now();
    for index in 1..=5 {
        service
            .insert(&VoiceSessionsEntity {
                id: 0,
                user_id: 3000 + index as u64,
                guild_id: 777_777,
                channel_id: 9001,
                join_time: now,
                leave_time: now + Duration::hours(i64::from(index)),
                is_active: false,
            })
            .await
            .expect("insert offset session");
    }
    let leaderboard = service
        .get_leaderboard_withopt(&leaderboard_options(777_777, 2, 2))
        .await
        .expect("get offset leaderboard");
    assert_eq!(leaderboard.len(), 2);
    assert_eq!(leaderboard[0].user_id, 3003);
    assert_eq!(leaderboard[0].total_duration, 3 * 3600);
    assert_eq!(leaderboard[1].user_id, 3002);
    assert_eq!(leaderboard[1].total_duration, 2 * 3600);
});

voice_repository_test!(get_leaderboard_empty, |repo| {
    let service = make_service(&repo).await;
    let leaderboard = service
        .get_leaderboard_withopt(&leaderboard_options(888_888, 0, 10))
        .await
        .expect("get empty leaderboard");
    assert!(leaderboard.is_empty());
});

voice_repository_test!(disabled_guilds_cache_on_init, |repo| {
    repo.voice_settings
        .replace(&VoiceSettingsEntity {
            guild_id: 999_999.into(),
            enabled: false,
        })
        .await
        .expect("seed disabled guild");
    let service = make_service(&repo).await;
    assert!(!service.is_enabled(999_999).await);
    assert!(service.is_enabled(111_111).await);
});

voice_repository_test!(get_leaderboard_includes_active_sessions, |repo| {
    let service = make_service(&repo).await;
    let now = Utc::now();
    let sessions = [
        (2001, now - Duration::hours(2), now, false),
        (
            2002,
            now - Duration::hours(1),
            now - Duration::hours(1),
            true,
        ),
        (
            2003,
            now - Duration::minutes(30),
            now - Duration::minutes(30),
            true,
        ),
    ];
    for (user_id, join_time, leave_time, is_active) in sessions {
        service
            .insert(&VoiceSessionsEntity {
                id: 0,
                user_id,
                guild_id: 999_999,
                channel_id: 9001,
                join_time,
                leave_time,
                is_active,
            })
            .await
            .expect("insert active session");
    }
    let leaderboard = service
        .get_leaderboard_withopt(&leaderboard_options(999_999, 0, 10))
        .await
        .expect("get active leaderboard");
    assert_eq!(leaderboard.len(), 3);
    assert_eq!(leaderboard[0].user_id, 2001);
    assert!(leaderboard[0].total_duration >= 7_200);
    assert_eq!(leaderboard[1].user_id, 2002);
    assert!(leaderboard[1].total_duration >= 3_600);
    assert_eq!(leaderboard[2].user_id, 2003);
    assert!(leaderboard[2].total_duration >= 1_800);
});

voice_repository_test!(get_leaderboard_active_and_completed_mixed, |repo| {
    let service = make_service(&repo).await;
    let now = Utc::now();
    let sessions = [
        (
            3001,
            now - Duration::hours(3),
            now - Duration::hours(2),
            false,
        ),
        (
            3001,
            now - Duration::minutes(30),
            now - Duration::minutes(30),
            true,
        ),
        (
            3002,
            now - Duration::hours(4),
            now - Duration::hours(3),
            false,
        ),
        (
            3002,
            now - Duration::hours(2),
            now - Duration::hours(1),
            false,
        ),
    ];
    for (user_id, join_time, leave_time, is_active) in sessions {
        service
            .insert(&VoiceSessionsEntity {
                id: 0,
                user_id,
                guild_id: 888_888,
                channel_id: 9001,
                join_time,
                leave_time,
                is_active,
            })
            .await
            .expect("insert mixed session");
    }
    let leaderboard = service
        .get_leaderboard_withopt(&leaderboard_options(888_888, 0, 10))
        .await
        .expect("get mixed leaderboard");
    assert_eq!(leaderboard.len(), 2);
    assert_eq!(leaderboard[0].user_id, 3002);
    assert_eq!(leaderboard[0].total_duration, 7_200);
    assert_eq!(leaderboard[1].user_id, 3001);
    assert!(leaderboard[1].total_duration >= 5_400);
});

voice_repository_test!(track_existing_user_already_tracked, |repo| {
    let service = Arc::new(make_service(&repo).await);
    let subscriber = VoiceStateSubscriber::new(service.clone());
    subscriber
        .track_existing_user(123, 456, 789, "session1")
        .await
        .expect("track existing user");
    subscriber
        .track_existing_user(123, 456, 789, "session1")
        .await
        .expect("deduplicate existing user");
    assert_eq!(repo.voice_sessions.select_all().await.unwrap().len(), 1);
});

voice_repository_test!(
    concurrent_join_and_snapshot_reserve_one_active_session,
    |repo| {
        let service = Arc::new(make_service(&repo).await);
        let subscriber = Arc::new(VoiceStateSubscriber::new(service.clone()));
        let event = VoiceStateEvent {
            old: None,
            new: VoiceState {
                user_id: 777,
                guild_id: Some(888),
                channel_id: Some(999),
                session_id: "shared-session".into(),
            },
        };
        let snapshot = json!({
            "guild": {
                "id": "888",
                "members": {},
                "voice_states": {
                    "777": {
                        "user_id": "777",
                        "channel_id": "999",
                        "session_id": "shared-session"
                    }
                }
            }
        });

        let (join, snapshot_result) = tokio::join!(
            subscriber.callback(event),
            subscriber.callback_guild_create(&snapshot),
        );
        join.expect("join event");
        snapshot_result.expect("guild snapshot");

        let active = service
            .find_active_sessions_by_user(777, 888)
            .await
            .expect("read active sessions");
        assert_eq!(
            active.len(),
            1,
            "concurrent events create one active session"
        );
    }
);

voice_repository_test!(track_existing_user_dedups_after_close_orphaned, |repo| {
    let service = Arc::new(make_service(&repo).await);
    let subscriber = VoiceStateSubscriber::new(service.clone());
    let orphaned = VoiceSessionsEntity {
        id: 0,
        user_id: 999,
        guild_id: 888,
        channel_id: 777,
        join_time: Utc::now() - Duration::hours(2),
        leave_time: Utc::now() - Duration::hours(2),
        is_active: true,
    };
    repo.voice_sessions
        .insert(&orphaned)
        .await
        .expect("insert orphan");

    subscriber
        .track_existing_user(999, 888, 777, "race_session")
        .await
        .expect("close orphan and track");
    let first = service
        .find_active_sessions_by_user(999, 888)
        .await
        .unwrap();
    assert_eq!(first.len(), 1);

    subscriber
        .track_existing_user(999, 888, 777, "race_session")
        .await
        .expect("deduplicate after close");
    let second = service
        .find_active_sessions_by_user(999, 888)
        .await
        .unwrap();
    assert_eq!(second.len(), 1);
    assert_eq!(second[0].join_time, first[0].join_time);
});

voice_repository_test!(handle_join_closes_orphaned_sessions, |repo| {
    let service = Arc::new(make_service(&repo).await);
    let subscriber = VoiceStateSubscriber::new(service.clone());
    let orphaned = VoiceSessionsEntity {
        id: 0,
        user_id: 444,
        guild_id: 555,
        channel_id: 789,
        join_time: Utc::now() - Duration::hours(2),
        leave_time: Utc::now() - Duration::hours(2),
        is_active: true,
    };
    repo.voice_sessions
        .insert(&orphaned)
        .await
        .expect("insert orphan");
    assert_eq!(
        service
            .find_active_sessions_by_user(444, 555)
            .await
            .unwrap()
            .len(),
        1
    );

    subscriber
        .callback(VoiceStateEvent {
            old: None,
            new: VoiceState {
                user_id: 444,
                guild_id: Some(555),
                channel_id: Some(789),
                session_id: "session1".into(),
            },
        })
        .await
        .expect("join closes orphan");
    let active = service
        .find_active_sessions_by_user(444, 555)
        .await
        .unwrap();
    assert_eq!(active.len(), 1);
    assert!(active[0].join_time > orphaned.join_time);
});

voice_repository_test!(guild_create_tracks_humans_and_filters_bots, |repo| {
    let service = Arc::new(make_service(&repo).await);
    let subscriber = VoiceStateSubscriber::new(service.clone());
    subscriber
        .callback_guild_create(&json!({
            "guild": {
                "id": "42",
                "members": {
                    "1": { "user": { "id": "1", "bot": false } },
                    "2": { "user": { "id": "2", "bot": true } }
                },
                "voice_states": {
                    "1": { "user_id": "1", "channel_id": "9", "session_id": "human" },
                    "2": { "user_id": "2", "channel_id": "10", "session_id": "bot" },
                    "3": { "user_id": "3", "channel_id": null, "session_id": "left" }
                }
            }
        }))
        .await
        .expect("guild snapshot");
    let active = service.find_active_sessions_by_user(1, 42).await.unwrap();
    assert_eq!(active.len(), 1);
    assert!(
        service
            .find_active_sessions_by_user(2, 42)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        service
            .find_active_sessions_by_user(3, 42)
            .await
            .unwrap()
            .is_empty()
    );
});

voice_repository_test!(handle_join_logic, |repo| {
    let service = Arc::new(make_service(&repo).await);
    let subscriber = VoiceStateSubscriber::new(service.clone());
    subscriber
        .callback(VoiceStateEvent {
            old: None,
            new: VoiceState {
                user_id: 123,
                guild_id: Some(456),
                channel_id: Some(789),
                session_id: "session1".into(),
            },
        })
        .await
        .expect("join event");
    assert_eq!(
        service
            .find_active_sessions_by_user(123, 456)
            .await
            .unwrap()
            .len(),
        1
    );
});

voice_repository_test!(handle_leave_logic, |repo| {
    let service = Arc::new(make_service(&repo).await);
    let subscriber = VoiceStateSubscriber::new(service.clone());
    let old = VoiceState {
        user_id: 123,
        guild_id: Some(456),
        channel_id: Some(789),
        session_id: "session1".into(),
    };
    subscriber
        .callback(VoiceStateEvent {
            old: None,
            new: old.clone(),
        })
        .await
        .expect("join before leave");
    subscriber
        .callback(VoiceStateEvent {
            old: Some(old),
            new: VoiceState {
                user_id: 123,
                guild_id: Some(456),
                channel_id: None,
                session_id: "session1".into(),
            },
        })
        .await
        .expect("leave event");
    assert!(
        service
            .find_active_sessions_by_user(123, 456)
            .await
            .unwrap()
            .is_empty()
    );
});

voice_repository_test!(handle_move_logic, |repo| {
    let service = Arc::new(make_service(&repo).await);
    let subscriber = VoiceStateSubscriber::new(service.clone());
    let old = VoiceState {
        user_id: 123,
        guild_id: Some(456),
        channel_id: Some(781),
        session_id: "session1".into(),
    };
    subscriber
        .callback(VoiceStateEvent {
            old: None,
            new: old.clone(),
        })
        .await
        .expect("join before move");
    subscriber
        .callback(VoiceStateEvent {
            old: Some(old),
            new: VoiceState {
                user_id: 123,
                guild_id: Some(456),
                channel_id: Some(782),
                session_id: "session2".into(),
            },
        })
        .await
        .expect("move event");
    let active = service
        .find_active_sessions_by_user(123, 456)
        .await
        .unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].channel_id, 782);
});

voice_repository_test!(track_existing_user, |repo| {
    let service = Arc::new(make_service(&repo).await);
    let subscriber = VoiceStateSubscriber::new(service.clone());
    subscriber
        .track_existing_user(123, 456, 789, "session1")
        .await
        .expect("track existing user");
    assert_eq!(
        service
            .find_active_sessions_by_user(123, 456)
            .await
            .unwrap()
            .len(),
        1
    );
});

voice_repository_test!(handle_leave_closes_all_active_sessions, |repo| {
    let service = Arc::new(make_service(&repo).await);
    let subscriber = VoiceStateSubscriber::new(service.clone());
    for channel_id in [789, 790, 791] {
        service
            .insert(&VoiceSessionsEntity {
                id: 0,
                user_id: 555,
                guild_id: 666,
                channel_id,
                join_time: Utc::now() - Duration::hours(1),
                leave_time: Utc::now() - Duration::hours(1),
                is_active: true,
            })
            .await
            .expect("insert duplicate active session");
    }
    subscriber
        .callback(VoiceStateEvent {
            old: Some(VoiceState {
                user_id: 555,
                guild_id: Some(666),
                channel_id: Some(789),
                session_id: "session1".into(),
            }),
            new: VoiceState {
                user_id: 555,
                guild_id: Some(666),
                channel_id: None,
                session_id: "session1".into(),
            },
        })
        .await
        .expect("leave closes duplicates");
    assert!(
        service
            .find_active_sessions_by_user(555, 666)
            .await
            .unwrap()
            .is_empty()
    );
});

voice_repository_test!(handle_join_dedup_same_session_id, |repo| {
    let service = Arc::new(make_service(&repo).await);
    let subscriber = VoiceStateSubscriber::new(service.clone());
    let event = VoiceStateEvent {
        old: None,
        new: VoiceState {
            user_id: 444,
            guild_id: Some(555),
            channel_id: Some(789),
            session_id: "session1".into(),
        },
    };
    subscriber
        .callback(event.clone())
        .await
        .expect("first join");
    subscriber.callback(event).await.expect("duplicate join");
    assert_eq!(
        service
            .find_active_sessions_by_user(444, 555)
            .await
            .unwrap()
            .len(),
        1
    );
});

#[test]
fn voice_state_event_accepts_discord_string_ids() {
    let event: VoiceStateEvent = serde_json::from_value(serde_json::json!({
        "old": null,
        "new": {
            "user_id": "42",
            "guild_id": "7",
            "channel_id": "9",
            "session_id": "session-1"
        }
    }))
    .expect("decode voice state");
    assert_eq!(event.new.user_id, 42);
    assert_eq!(event.new.guild_id, Some(7));
    assert_eq!(event.new.channel_id, Some(9));
}

#[tokio::test]
#[serial_test::serial]
async fn voice_subscriber_tracks_join_move_and_leave_events() {
    let db_url = db::db_url().await;
    let repository = Repository::connect(&db_url)
        .await
        .expect("connect voice storage");
    repository.migrate().await.expect("run voice migrations");
    repository
        .delete_all()
        .await
        .expect("start with empty tables");
    let service = Arc::new(
        VoiceTrackingService::new(
            Arc::new(repository.voice_sessions.clone()),
            Arc::new(repository.voice_settings.clone()),
        )
        .await
        .expect("construct voice service"),
    );
    let subscriber = VoiceStateSubscriber::new(service.clone());

    let join = VoiceStateEvent {
        old: None,
        new: VoiceState {
            user_id: 42,
            guild_id: Some(7),
            channel_id: Some(9),
            session_id: "session-1".into(),
        },
    };
    subscriber.callback(join.clone()).await.expect("join event");
    assert_eq!(
        service
            .find_active_sessions_by_user(42, 7)
            .await
            .unwrap()
            .len(),
        1
    );

    let move_event = VoiceStateEvent {
        old: Some(join.new),
        new: VoiceState {
            user_id: 42,
            guild_id: Some(7),
            channel_id: Some(10),
            session_id: "session-2".into(),
        },
    };
    subscriber
        .callback(move_event.clone())
        .await
        .expect("move event");
    let active = service.find_active_sessions_by_user(42, 7).await.unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].channel_id, 10);

    subscriber
        .callback(VoiceStateEvent {
            old: Some(move_event.new),
            new: VoiceState {
                user_id: 42,
                guild_id: Some(7),
                channel_id: None,
                session_id: "session-2".into(),
            },
        })
        .await
        .expect("leave event");
    assert!(
        service
            .find_active_sessions_by_user(42, 7)
            .await
            .unwrap()
            .is_empty()
    );

    service
        .update_settings(7, voice::VoiceSettings { enabled: false })
        .await
        .expect("disable voice tracking");
    subscriber
        .callback(VoiceStateEvent {
            old: None,
            new: VoiceState {
                user_id: 99,
                guild_id: Some(7),
                channel_id: Some(9),
                session_id: "disabled-session".into(),
            },
        })
        .await
        .expect("disabled guild event");
    assert!(
        service
            .find_active_sessions_by_user(99, 7)
            .await
            .unwrap()
            .is_empty()
    );
}
