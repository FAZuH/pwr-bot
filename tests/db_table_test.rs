//! Integration tests for database table operations.

use chrono::Duration;
use chrono::Utc;
use pwr_bot::entity::FeedEntity;
use pwr_bot::entity::FeedItemEntity;
use pwr_bot::entity::FeedSubscriptionEntity;
use pwr_bot::entity::FeedsSettings;
use pwr_bot::entity::ServerSettingsEntity;
use pwr_bot::entity::SubscriberEntity;
use pwr_bot::entity::SubscriberType;
use pwr_bot::entity::VoiceSessionsEntity;
use pwr_bot::entity::WelcomeSettings;
use pwr_bot::repository::table::Table;

mod common;

// --- 1. Test Harness Macro ---
// Handles setup, execution, and teardown automatically.
macro_rules! db_test {
    ($name:ident, |$db:ident| $body:block) => {
        #[tokio::test]
        async fn $name() {
            let ($db, db_path) = common::setup_db().await;

            // Execute the test logic
            $body

            common::teardown_db(db_path).await;
        }
    };
}

// --- 2. Data Fixture Macros ---
// Helpers to quickly insert data with defaults, allowing overrides.

macro_rules! create_feed {
    ($db:expr, $name:expr) => {
        create_feed!($db, $name, {})
    };
    ($db:expr, $name:expr, { $($field:ident : $val:expr),* }) => {
        {
            #[allow(unused_mut)]
            let mut feed = FeedEntity {
                name: $name.to_string(),
                platform_id: $name.replace(" ", "").to_lowercase(),
                source_url: format!("https://{}.com", $name.replace(" ", "").to_lowercase()),
                ..Default::default()
            };
            $(feed.$field = $val.into();)*
            $db.feed.insert(&feed).await.expect("Failed to insert feed")
        }
    };
}

macro_rules! create_sub {
    ($db:expr, $target:expr) => {
        $db.subscriber
            .insert(&SubscriberEntity {
                r#type: SubscriberType::Dm,
                target_id: $target.to_string(),
                ..Default::default()
            })
            .await
            .expect("Failed to insert subscriber")
    };
}

macro_rules! create_subscription {
    ($db:expr, $feed_id:expr, $sub_id:expr) => {
        $db.feed_subscription
            .insert(&FeedSubscriptionEntity {
                feed_id: $feed_id,
                subscriber_id: $sub_id,
                ..Default::default()
            })
            .await
            .expect("Failed to subscribe")
    };
}

macro_rules! create_item {
    ($db:expr, $feed_id:expr, $desc:expr) => {
        create_item!($db, $feed_id, $desc, Utc::now())
    };
    ($db:expr, $feed_id:expr, $desc:expr, $date:expr) => {
        $db.feed_item
            .insert(&FeedItemEntity {
                feed_id: $feed_id,
                description: $desc.to_string(),
                published: $date,
                ..Default::default()
            })
            .await
            .expect("Failed to insert item")
    };
}

macro_rules! create_voice_session {
    ($db:expr, $user:expr, $guild:expr, $chan:expr) => {
        $db.voice_sessions
            .insert(&VoiceSessionsEntity {
                user_id: $user,
                guild_id: $guild,
                channel_id: $chan,
                join_time: Utc::now(),
                leave_time: Utc::now() + Duration::hours(1),
                is_active: false,
                ..Default::default()
            })
            .await
            .expect("Failed to insert voice session")
    };
}

// --- 3. Refactored Tests ---

mod feed_table_tests {
    use super::*;

    db_test!(insert_and_select, |db| {
        let id = create_feed!(db, "Test Feed", { description: "Test Description" });
        assert!(id > 0);

        let fetched = db.feed.select(&id).await.unwrap().unwrap();
        assert_eq!(fetched.name, "Test Feed");
    });

    db_test!(update, |db| {
        let id = create_feed!(db, "Original");
        let mut data = db.feed.select(&id).await.unwrap().unwrap();

        data.name = "Updated".to_string();
        db.feed.update(&data).await.expect("Failed to update");

        let fetched = db.feed.select(&id).await.unwrap().unwrap();
        assert_eq!(fetched.name, "Updated");
    });

    db_test!(replace, |db| {
        let id = create_feed!(db, "Original", { description: "Old" });
        let mut data = db.feed.select(&id).await.unwrap().unwrap();

        data.description = "Replaced".to_string();
        let new_id = db.feed.replace(&data).await.expect("Failed to replace");

        let fetched = db.feed.select(&new_id).await.unwrap().unwrap();
        assert_eq!(fetched.description, "Replaced");
    });

    db_test!(delete, |db| {
        let id = create_feed!(db, "Test");
        db.feed.delete(&id).await.expect("Failed to delete");
        assert!(db.feed.select(&id).await.unwrap().is_none());
    });

    db_test!(select_by_source_id, |db| {
        create_feed!(db, "Feed", { platform_id: "anilist", source_id: "frieren" });
        let fetched = db
            .feed
            .select_by_source_id("anilist", "frieren")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.name, "Feed");
    });

    db_test!(select_all_by_tag, |db| {
        create_feed!(db, "Feed 1", { tags: "manga,shonen" });
        create_feed!(db, "Feed 2", { tags: "anime,shonen" });

        let shonen = db.feed.select_all_by_tag("shonen").await.unwrap();
        assert_eq!(shonen.len(), 2);

        let anime = db.feed.select_all_by_tag("anime").await.unwrap();
        assert_eq!(anime.len(), 1);
        assert_eq!(anime[0].name, "Feed 2");
    });

    db_test!(select_by_name_and_subscriber_id, |db| {
        let sub_id = create_sub!(db, "user1");
        let f1 = create_feed!(db, "One Piece");
        let f2 = create_feed!(db, "One Punch Man");
        let _f3 = create_feed!(db, "Naruto");

        create_subscription!(db, f1, sub_id);
        create_subscription!(db, f2, sub_id);

        // Search "one" (should match 2)
        let res = db
            .feed
            .select_by_name_and_subscriber_id(&sub_id, "one", None)
            .await
            .unwrap();
        assert_eq!(res.len(), 2);

        // Search "Naruto" (should match 0 as not subscribed)
        let res = db
            .feed
            .select_by_name_and_subscriber_id(&sub_id, "Naruto", None)
            .await
            .unwrap();
        assert_eq!(res.len(), 0);

        // Test limit
        let res = db
            .feed
            .select_by_name_and_subscriber_id(&sub_id, "One", 1u32)
            .await
            .unwrap();
        assert_eq!(res.len(), 1);
    });
}

mod feed_item_table_tests {
    use super::*;

    db_test!(insert_and_select_latest, |db| {
        let feed_id = create_feed!(db, "Feed");
        create_item!(db, feed_id, "Chapter 1", Utc::now());
        create_item!(db, feed_id, "Chapter 2", Utc::now() + Duration::hours(1));

        let latest = db
            .feed_item
            .select_latest_by_feed_id(feed_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(latest.description, "Chapter 2");
    });

    db_test!(select_all_by_feed_id_ordered, |db| {
        let feed_id = create_feed!(db, "Feed");
        create_item!(db, feed_id, "Chapter 1", Utc::now());
        create_item!(db, feed_id, "Chapter 2", Utc::now() + Duration::hours(1));

        let all = db.feed_item.select_all_by_feed_id(feed_id).await.unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].description, "Chapter 2"); // Ordered by published desc
    });

    db_test!(update, |db| {
        let feed_id = create_feed!(db, "Feed");
        let item_id = create_item!(db, feed_id, "Original");

        let mut item = db.feed_item.select(&item_id).await.unwrap().unwrap();
        item.description = "Updated".to_string();
        db.feed_item.update(&item).await.unwrap();

        let fetched = db.feed_item.select(&item_id).await.unwrap().unwrap();
        assert_eq!(fetched.description, "Updated");
    });

    db_test!(delete_all_by_feed_id, |db| {
        let feed_id = create_feed!(db, "Feed");
        create_item!(db, feed_id, "Item 1");
        create_item!(db, feed_id, "Item 2");

        db.feed_item.delete_all_by_feed_id(feed_id).await.unwrap();

        let all = db.feed_item.select_all_by_feed_id(feed_id).await.unwrap();
        assert!(all.is_empty());
    });
}

mod subscriber_table_tests {
    use super::*;

    db_test!(insert_and_select_by_type_and_target, |db| {
        let id = create_sub!(db, "user1");
        let fetched = db
            .subscriber
            .select_by_type_and_target(&SubscriberType::Dm, "user1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.id, id);
    });

    db_test!(select_all_by_type_and_feed, |db| {
        let sub_id = create_sub!(db, "user1");
        let feed_id = create_feed!(db, "Feed");
        create_subscription!(db, feed_id, sub_id);

        let subs = db
            .subscriber
            .select_all_by_type_and_feed(SubscriberType::Dm, feed_id)
            .await
            .unwrap();

        assert_eq!(subs.len(), 1);
        assert_eq!(subs[0].target_id, "user1");
    });
}

mod feed_subscription_table_tests {
    use super::*;

    db_test!(insert_and_exists, |db| {
        let f_id = create_feed!(db, "Feed");
        let s_id = create_sub!(db, "u1");
        create_subscription!(db, f_id, s_id);

        assert!(db.feed_subscription.exists_by_feed_id(f_id).await.unwrap());
    });

    db_test!(count_by_subscriber_id, |db| {
        let f_id = create_feed!(db, "Feed");
        let s_id = create_sub!(db, "u1");
        create_subscription!(db, f_id, s_id);

        let count = db
            .feed_subscription
            .count_by_subscriber_id(s_id)
            .await
            .unwrap();
        assert_eq!(count, 1);
    });

    db_test!(select_all_by_feed_id, |db| {
        let f_id = create_feed!(db, "Feed");
        let s_id = create_sub!(db, "u1");
        create_subscription!(db, f_id, s_id);

        let subs = db
            .feed_subscription
            .select_all_by_feed_id(f_id)
            .await
            .unwrap();
        assert_eq!(subs.len(), 1);
    });

    db_test!(select_all_by_subscriber_id, |db| {
        let f_id = create_feed!(db, "Feed");
        let s_id = create_sub!(db, "u1");
        create_subscription!(db, f_id, s_id);

        let subs = db
            .feed_subscription
            .select_all_by_subscriber_id(s_id)
            .await
            .unwrap();
        assert_eq!(subs.len(), 1);
    });

    db_test!(select_paginated, |db| {
        let f_id = create_feed!(db, "Feed");
        let s_id = create_sub!(db, "u1");
        create_subscription!(db, f_id, s_id);

        let page = db
            .feed_subscription
            .select_paginated_by_subscriber_id(s_id, 0u32, 10u32)
            .await
            .unwrap();
        assert_eq!(page.len(), 1);
    });

    db_test!(select_paginated_with_latest, |db| {
        let f_id = create_feed!(db, "Feed");
        let s_id = create_sub!(db, "u1");
        create_subscription!(db, f_id, s_id);
        create_item!(db, f_id, "Latest Item");

        let page = db
            .feed_subscription
            .select_paginated_with_latest_by_subscriber_id(s_id, 0u32, 10u32)
            .await
            .unwrap();
        assert_eq!(page.len(), 1);
        assert_eq!(page[0].name, "Feed");
        assert_eq!(page[0].item_description, Some("Latest Item".to_string()));
    });

    db_test!(delete_subscription, |db| {
        let f_id = create_feed!(db, "Feed");
        let s_id = create_sub!(db, "u1");
        create_subscription!(db, f_id, s_id);

        db.feed_subscription
            .delete_subscription(f_id, s_id)
            .await
            .unwrap();
        assert!(!db.feed_subscription.exists_by_feed_id(f_id).await.unwrap());
    });

    db_test!(delete_all_by_subscriber_id, |db| {
        let f_id = create_feed!(db, "Feed");
        let s_id = create_sub!(db, "u1");
        create_subscription!(db, f_id, s_id);

        db.feed_subscription
            .delete_all_by_subscriber_id(s_id)
            .await
            .unwrap();

        let count = db
            .feed_subscription
            .count_by_subscriber_id(s_id)
            .await
            .unwrap();
        assert_eq!(count, 0);
    });

    db_test!(delete_all_by_feed_id, |db| {
        let f_id = create_feed!(db, "Feed");
        let s_id = create_sub!(db, "u1");
        create_subscription!(db, f_id, s_id);

        db.feed_subscription
            .delete_all_by_feed_id(f_id)
            .await
            .unwrap();
        assert!(!db.feed_subscription.exists_by_feed_id(f_id).await.unwrap());
    });
}

mod server_settings_table_tests {
    use pwr_bot::entity::ServerSettings;
    use pwr_bot::entity::VoiceSettings;

    use super::*;

    fn create_settings(guild_id: u64, chan: &str) -> ServerSettingsEntity {
        ServerSettingsEntity {
            guild_id,
            settings: sqlx::types::Json(ServerSettings {
                voice: VoiceSettings::default(),
                feeds: FeedsSettings {
                    enabled: Some(true),
                    channel_id: Some(chan.to_string()),
                    subscribe_role_id: None,
                    unsubscribe_role_id: None,
                },
                welcome: WelcomeSettings::default(),
            }),
        }
    }

    db_test!(insert_and_select, |db| {
        let id = db
            .server_settings
            .insert(&create_settings(123, "c1"))
            .await
            .unwrap();
        assert_eq!(id, 123);

        let fetched = db.server_settings.select(&123).await.unwrap().unwrap();
        assert_eq!(fetched.settings.0.feeds.channel_id, Some("c1".to_string()));
    });

    db_test!(update, |db| {
        db.server_settings
            .insert(&create_settings(123, "c1"))
            .await
            .unwrap();

        let updated = create_settings(123, "c2");
        db.server_settings.update(&updated).await.unwrap();

        let fetched = db.server_settings.select(&123).await.unwrap().unwrap();
        assert_eq!(fetched.settings.0.feeds.channel_id, Some("c2".to_string()));
    });

    db_test!(delete, |db| {
        db.server_settings
            .insert(&create_settings(123, "c1"))
            .await
            .unwrap();

        db.server_settings.delete(&123).await.unwrap();
        assert!(db.server_settings.select(&123).await.unwrap().is_none());
    });
}

mod voice_sessions_table_tests {
    use super::*;

    db_test!(insert_and_select, |db| {
        let id = create_voice_session!(db, 1, 2, 3);
        assert!(id > 0);

        let fetched = db.voice_sessions.select(&id).await.unwrap().unwrap();
        assert_eq!(fetched.user_id, 1);
        assert_eq!(fetched.guild_id, 2);
        assert_eq!(fetched.channel_id, 3);
    });

    db_test!(update, |db| {
        let id = create_voice_session!(db, 1, 2, 3);
        let mut session = db.voice_sessions.select(&id).await.unwrap().unwrap();

        session.channel_id = 4;
        db.voice_sessions.update(&session).await.unwrap();

        let fetched = db.voice_sessions.select(&id).await.unwrap().unwrap();
        assert_eq!(fetched.channel_id, 4);
    });

    db_test!(delete, |db| {
        let id = create_voice_session!(db, 1, 2, 3);
        db.voice_sessions.delete(&id).await.unwrap();
        assert!(db.voice_sessions.select(&id).await.unwrap().is_none());
    });

    db_test!(update_leave_time, |db| {
        let join_time = Utc::now();
        let session = VoiceSessionsEntity {
            id: 0,
            user_id: 100,
            guild_id: 200,
            channel_id: 300,
            join_time,
            leave_time: join_time, // Active session
            is_active: true,
        };
        db.voice_sessions
            .insert(&session)
            .await
            .expect("Failed to insert session");

        // Update leave_time
        let new_leave_time = join_time + Duration::hours(1);
        db.voice_sessions
            .update_leave_time(100, 300, &join_time, &new_leave_time)
            .await
            .expect("Failed to update leave time");

        // Verify the update
        let sessions: Vec<VoiceSessionsEntity> = db
            .voice_sessions
            .select_all()
            .await
            .expect("Failed to select sessions");
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].leave_time, new_leave_time);
        assert_ne!(sessions[0].leave_time, sessions[0].join_time);
    });

    db_test!(find_active_sessions, |db| {
        let now = Utc::now();

        // Insert mix of active and completed sessions
        let active_session = VoiceSessionsEntity {
            id: 0,
            user_id: 100,
            guild_id: 200,
            channel_id: 300,
            join_time: now - Duration::hours(1),
            leave_time: now - Duration::hours(1), // Active
            is_active: true,
        };
        db.voice_sessions
            .insert(&active_session)
            .await
            .expect("Failed to insert active session");

        let completed_session = VoiceSessionsEntity {
            id: 0,
            user_id: 101,
            guild_id: 200,
            channel_id: 301,
            join_time: now - Duration::hours(2),
            leave_time: now - Duration::hours(1), // Completed
            is_active: false,
        };
        db.voice_sessions
            .insert(&completed_session)
            .await
            .expect("Failed to insert completed session");

        let another_active = VoiceSessionsEntity {
            id: 0,
            user_id: 102,
            guild_id: 200,
            channel_id: 302,
            join_time: now - Duration::minutes(30),
            leave_time: now - Duration::minutes(30), // Active
            is_active: true,
        };
        db.voice_sessions
            .insert(&another_active)
            .await
            .expect("Failed to insert another active session");

        // Find active sessions
        let active = db
            .voice_sessions
            .find_active_sessions()
            .await
            .expect("Failed to find active sessions");

        // Should find exactly 2 active sessions
        assert_eq!(active.len(), 2);

        // Verify correct users are found
        let user_ids: Vec<u64> = active.iter().map(|s| s.user_id).collect();
        assert!(user_ids.contains(&100), "User 100 should be active");
        assert!(user_ids.contains(&102), "User 102 should be active");
        assert!(!user_ids.contains(&101), "User 101 should not be active");
    });

    db_test!(find_active_sessions_empty, |db| {
        // No sessions inserted
        let active = db
            .voice_sessions
            .find_active_sessions()
            .await
            .expect("Failed to find active sessions");

        // Should return empty vector
        assert!(active.is_empty());
    });

    db_test!(update_leave_time_no_match, |db| {
        let join_time = Utc::now();

        // Try to update a session that doesn't exist
        let new_leave_time = join_time + Duration::hours(1);
        let result = db
            .voice_sessions
            .update_leave_time(999, 999, &join_time, &new_leave_time)
            .await;

        // Should not error, just not update anything
        assert!(result.is_ok());

        // Verify no sessions exist
        let sessions: Vec<VoiceSessionsEntity> = db
            .voice_sessions
            .select_all()
            .await
            .expect("Failed to select sessions");
        assert!(sessions.is_empty());
    });

    db_test!(get_user_daily_activity, |db| {
        let now = Utc::now();
        let today = now.date_naive();
        let yesterday = today - Duration::days(1);

        // Insert sessions for user 100 on different days
        let session1 = VoiceSessionsEntity {
            id: 0,
            user_id: 100,
            guild_id: 200,
            channel_id: 300,
            join_time: now,
            leave_time: now + Duration::hours(2), // 2 hours today
            is_active: false,
        };
        db.voice_sessions
            .insert(&session1)
            .await
            .expect("Failed to insert session 1");

        let session2 = VoiceSessionsEntity {
            id: 0,
            user_id: 100,
            guild_id: 200,
            channel_id: 301,
            join_time: now - Duration::days(1),
            leave_time: now - Duration::days(1) + Duration::hours(1), // 1 hour yesterday
            is_active: false,
        };
        db.voice_sessions
            .insert(&session2)
            .await
            .expect("Failed to insert session 2");

        // Insert session for different user (should not appear in results)
        let session3 = VoiceSessionsEntity {
            id: 0,
            user_id: 101,
            guild_id: 200,
            channel_id: 302,
            join_time: now,
            leave_time: now + Duration::minutes(30),
            is_active: false,
        };
        db.voice_sessions
            .insert(&session3)
            .await
            .expect("Failed to insert session 3");

        // Get daily activity for user 100
        let since = now - Duration::days(2);
        let until = now + Duration::days(1);
        let activity = db
            .voice_sessions
            .get_user_daily_activity(100, 200, &since, &until)
            .await
            .expect("Failed to get user daily activity");

        // Should have 2 days of activity
        assert_eq!(activity.len(), 2, "Should have 2 days of activity");

        // Check today's activity (7200 seconds = 2 hours)
        let today_activity = activity
            .iter()
            .find(|a| a.day == today)
            .expect("Should have today's activity");
        assert_eq!(
            today_activity.total_seconds, 7200,
            "Today should have 2 hours"
        );

        // Check yesterday's activity (3600 seconds = 1 hour)
        let yesterday_activity = activity
            .iter()
            .find(|a| a.day == yesterday)
            .expect("Should have yesterday's activity");
        assert_eq!(
            yesterday_activity.total_seconds, 3600,
            "Yesterday should have 1 hour"
        );
    });

    db_test!(get_user_daily_activity_empty, |db| {
        let now = Utc::now();
        let since = now - Duration::days(7);
        let until = now;

        // Get activity for user with no sessions
        let activity = db
            .voice_sessions
            .get_user_daily_activity(999, 200, &since, &until)
            .await
            .expect("Failed to get user daily activity");

        assert!(
            activity.is_empty(),
            "Should return empty for user with no activity"
        );
    });

    db_test!(get_guild_daily_average_time, |db| {
        let now = Utc::now();

        // User 100: 2 hours today
        let session1 = VoiceSessionsEntity {
            id: 0,
            user_id: 100,
            guild_id: 200,
            channel_id: 300,
            join_time: now,
            leave_time: now + Duration::hours(2),
            is_active: false,
        };
        db.voice_sessions
            .insert(&session1)
            .await
            .expect("Failed to insert session 1");

        // User 101: 1 hour today (same guild)
        let session2 = VoiceSessionsEntity {
            id: 0,
            user_id: 101,
            guild_id: 200,
            channel_id: 301,
            join_time: now,
            leave_time: now + Duration::hours(1),
            is_active: false,
        };
        db.voice_sessions
            .insert(&session2)
            .await
            .expect("Failed to insert session 2");

        // User 102: 30 minutes today (different guild - should not appear)
        let session3 = VoiceSessionsEntity {
            id: 0,
            user_id: 102,
            guild_id: 999,
            channel_id: 302,
            join_time: now,
            leave_time: now + Duration::minutes(30),
            is_active: false,
        };
        db.voice_sessions
            .insert(&session3)
            .await
            .expect("Failed to insert session 3");

        // Get guild daily average time
        let since = now - Duration::days(1);
        let until = now + Duration::days(1);
        let stats = db
            .voice_sessions
            .get_guild_daily_average_time(200, &since, &until)
            .await
            .expect("Failed to get guild daily average time");

        assert_eq!(stats.len(), 1, "Should have 1 day of stats");
        // Average of 2 hours (7200s) and 1 hour (3600s) = 1.5 hours (5400s)
        assert_eq!(stats[0].value, 5400, "Average should be 1.5 hours");
    });

    db_test!(get_guild_daily_user_count, |db| {
        let now = Utc::now();
        let today = now.date_naive();

        // User 100: active today
        let session1 = VoiceSessionsEntity {
            id: 0,
            user_id: 100,
            guild_id: 200,
            channel_id: 300,
            join_time: now,
            leave_time: now + Duration::hours(1),
            is_active: false,
        };
        db.voice_sessions
            .insert(&session1)
            .await
            .expect("Failed to insert session 1");

        // User 101: active today (same guild, different channel)
        let session2 = VoiceSessionsEntity {
            id: 0,
            user_id: 101,
            guild_id: 200,
            channel_id: 301,
            join_time: now,
            leave_time: now + Duration::hours(2),
            is_active: false,
        };
        db.voice_sessions
            .insert(&session2)
            .await
            .expect("Failed to insert session 2");

        // User 100: another session today (should not double count)
        let session3 = VoiceSessionsEntity {
            id: 0,
            user_id: 100,
            guild_id: 200,
            channel_id: 302,
            join_time: now + Duration::minutes(30),
            leave_time: now + Duration::minutes(90),
            is_active: false,
        };
        db.voice_sessions
            .insert(&session3)
            .await
            .expect("Failed to insert session 3");

        // Get guild daily user count
        let since = now - Duration::days(1);
        let until = now + Duration::days(1);
        let stats = db
            .voice_sessions
            .get_guild_daily_user_count(200, &since, &until)
            .await
            .expect("Failed to get guild daily user count");

        assert_eq!(stats.len(), 1, "Should have 1 day of stats");
        assert_eq!(stats[0].day, today, "Day should be today");
        assert_eq!(stats[0].value, 2, "Should have 2 unique users");
    });

    db_test!(get_guild_daily_stats_empty, |db| {
        let now = Utc::now();
        let since = now - Duration::days(7);
        let until = now;

        // Get average time for guild with no sessions
        let avg_stats = db
            .voice_sessions
            .get_guild_daily_average_time(999, &since, &until)
            .await
            .expect("Failed to get guild daily average time");
        assert!(
            avg_stats.is_empty(),
            "Should return empty for guild with no activity"
        );

        // Get user count for guild with no sessions
        let count_stats = db
            .voice_sessions
            .get_guild_daily_user_count(999, &since, &until)
            .await
            .expect("Failed to get guild daily user count");
        assert!(
            count_stats.is_empty(),
            "Should return empty for guild with no activity"
        );
    });
}
