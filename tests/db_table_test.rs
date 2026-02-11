//! Integration tests for database table operations.

use chrono::Duration;
use chrono::Utc;
use pwr_bot::database::model::FeedItemModel;
use pwr_bot::database::model::FeedModel;
use pwr_bot::database::model::FeedSubscriptionModel;
use pwr_bot::database::model::ServerSettings;
use pwr_bot::database::model::ServerSettingsModel;
use pwr_bot::database::model::SubscriberModel;
use pwr_bot::database::model::SubscriberType;
use pwr_bot::database::model::VoiceSessionsModel;
use pwr_bot::database::table::Table;

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
            let mut feed = FeedModel {
                name: $name.to_string(),
                platform_id: $name.replace(" ", "").to_lowercase(),
                source_url: format!("https://{}.com", $name.replace(" ", "").to_lowercase()),
                ..Default::default()
            };
            $(feed.$field = $val.into();)*
            $db.feed_table.insert(&feed).await.expect("Failed to insert feed")
        }
    };
}

macro_rules! create_sub {
    ($db:expr, $target:expr) => {
        $db.subscriber_table
            .insert(&SubscriberModel {
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
        $db.feed_subscription_table
            .insert(&FeedSubscriptionModel {
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
        $db.feed_item_table
            .insert(&FeedItemModel {
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
        $db.voice_sessions_table
            .insert(&VoiceSessionsModel {
                user_id: $user,
                guild_id: $guild,
                channel_id: $chan,
                join_time: Utc::now(),
                leave_time: Utc::now() + Duration::hours(1),
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

        let fetched = db.feed_table.select(&id).await.unwrap().unwrap();
        assert_eq!(fetched.name, "Test Feed");
    });

    db_test!(update, |db| {
        let id = create_feed!(db, "Original");
        let mut data = db.feed_table.select(&id).await.unwrap().unwrap();

        data.name = "Updated".to_string();
        db.feed_table.update(&data).await.expect("Failed to update");

        let fetched = db.feed_table.select(&id).await.unwrap().unwrap();
        assert_eq!(fetched.name, "Updated");
    });

    db_test!(replace, |db| {
        let id = create_feed!(db, "Original", { description: "Old" });
        let mut data = db.feed_table.select(&id).await.unwrap().unwrap();

        data.description = "Replaced".to_string();
        let new_id = db
            .feed_table
            .replace(&data)
            .await
            .expect("Failed to replace");

        let fetched = db.feed_table.select(&new_id).await.unwrap().unwrap();
        assert_eq!(fetched.description, "Replaced");
    });

    db_test!(delete, |db| {
        let id = create_feed!(db, "Test");
        db.feed_table.delete(&id).await.expect("Failed to delete");
        assert!(db.feed_table.select(&id).await.unwrap().is_none());
    });

    db_test!(select_by_source_id, |db| {
        create_feed!(db, "Feed", { platform_id: "anilist", source_id: "frieren" });
        let fetched = db
            .feed_table
            .select_by_source_id("anilist", "frieren")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.name, "Feed");
    });

    db_test!(select_all_by_tag, |db| {
        create_feed!(db, "Feed 1", { tags: "manga,shonen" });
        create_feed!(db, "Feed 2", { tags: "anime,shonen" });

        let shonen = db.feed_table.select_all_by_tag("shonen").await.unwrap();
        assert_eq!(shonen.len(), 2);

        let anime = db.feed_table.select_all_by_tag("anime").await.unwrap();
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
            .feed_table
            .select_by_name_and_subscriber_id(&sub_id, "one", None)
            .await
            .unwrap();
        assert_eq!(res.len(), 2);

        // Search "Naruto" (should match 0 as not subscribed)
        let res = db
            .feed_table
            .select_by_name_and_subscriber_id(&sub_id, "Naruto", None)
            .await
            .unwrap();
        assert_eq!(res.len(), 0);

        // Test limit
        let res = db
            .feed_table
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
            .feed_item_table
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

        let all = db
            .feed_item_table
            .select_all_by_feed_id(feed_id)
            .await
            .unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].description, "Chapter 2"); // Ordered by published desc
    });

    db_test!(update, |db| {
        let feed_id = create_feed!(db, "Feed");
        let item_id = create_item!(db, feed_id, "Original");

        let mut item = db.feed_item_table.select(&item_id).await.unwrap().unwrap();
        item.description = "Updated".to_string();
        db.feed_item_table.update(&item).await.unwrap();

        let fetched = db.feed_item_table.select(&item_id).await.unwrap().unwrap();
        assert_eq!(fetched.description, "Updated");
    });

    db_test!(delete_all_by_feed_id, |db| {
        let feed_id = create_feed!(db, "Feed");
        create_item!(db, feed_id, "Item 1");
        create_item!(db, feed_id, "Item 2");

        db.feed_item_table
            .delete_all_by_feed_id(feed_id)
            .await
            .unwrap();

        let all = db
            .feed_item_table
            .select_all_by_feed_id(feed_id)
            .await
            .unwrap();
        assert!(all.is_empty());
    });
}

mod subscriber_table_tests {
    use super::*;

    db_test!(insert_and_select_by_type_and_target, |db| {
        let id = create_sub!(db, "user1");
        let fetched = db
            .subscriber_table
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
            .subscriber_table
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

        assert!(
            db.feed_subscription_table
                .exists_by_feed_id(f_id)
                .await
                .unwrap()
        );
    });

    db_test!(count_by_subscriber_id, |db| {
        let f_id = create_feed!(db, "Feed");
        let s_id = create_sub!(db, "u1");
        create_subscription!(db, f_id, s_id);

        let count = db
            .feed_subscription_table
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
            .feed_subscription_table
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
            .feed_subscription_table
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
            .feed_subscription_table
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
            .feed_subscription_table
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

        db.feed_subscription_table
            .delete_subscription(f_id, s_id)
            .await
            .unwrap();
        assert!(
            !db.feed_subscription_table
                .exists_by_feed_id(f_id)
                .await
                .unwrap()
        );
    });

    db_test!(delete_all_by_subscriber_id, |db| {
        let f_id = create_feed!(db, "Feed");
        let s_id = create_sub!(db, "u1");
        create_subscription!(db, f_id, s_id);

        db.feed_subscription_table
            .delete_all_by_subscriber_id(s_id)
            .await
            .unwrap();

        let count = db
            .feed_subscription_table
            .count_by_subscriber_id(s_id)
            .await
            .unwrap();
        assert_eq!(count, 0);
    });

    db_test!(delete_all_by_feed_id, |db| {
        let f_id = create_feed!(db, "Feed");
        let s_id = create_sub!(db, "u1");
        create_subscription!(db, f_id, s_id);

        db.feed_subscription_table
            .delete_all_by_feed_id(f_id)
            .await
            .unwrap();
        assert!(
            !db.feed_subscription_table
                .exists_by_feed_id(f_id)
                .await
                .unwrap()
        );
    });
}

mod server_settings_table_tests {
    use super::*;

    fn create_settings(guild_id: u64, chan: &str) -> ServerSettingsModel {
        ServerSettingsModel {
            guild_id,
            settings: sqlx::types::Json(ServerSettings {
                enabled: Some(true),
                channel_id: Some(chan.to_string()),
                subscribe_role_id: None,
                unsubscribe_role_id: None,
                voice_tracking_enabled: None,
            }),
        }
    }

    db_test!(insert_and_select, |db| {
        let id = db
            .server_settings_table
            .insert(&create_settings(123, "c1"))
            .await
            .unwrap();
        assert_eq!(id, 123);

        let fetched = db
            .server_settings_table
            .select(&123)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.settings.0.channel_id, Some("c1".to_string()));
    });

    db_test!(update, |db| {
        db.server_settings_table
            .insert(&create_settings(123, "c1"))
            .await
            .unwrap();

        let updated = create_settings(123, "c2");
        db.server_settings_table.update(&updated).await.unwrap();

        let fetched = db
            .server_settings_table
            .select(&123)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.settings.0.channel_id, Some("c2".to_string()));
    });

    db_test!(delete, |db| {
        db.server_settings_table
            .insert(&create_settings(123, "c1"))
            .await
            .unwrap();

        db.server_settings_table.delete(&123).await.unwrap();
        assert!(
            db.server_settings_table
                .select(&123)
                .await
                .unwrap()
                .is_none()
        );
    });
}

mod voice_sessions_table_tests {
    use super::*;

    db_test!(insert_and_select, |db| {
        let id = create_voice_session!(db, 1, 2, 3);
        assert!(id > 0);

        let fetched = db.voice_sessions_table.select(&id).await.unwrap().unwrap();
        assert_eq!(fetched.user_id, 1);
        assert_eq!(fetched.guild_id, 2);
        assert_eq!(fetched.channel_id, 3);
    });

    db_test!(update, |db| {
        let id = create_voice_session!(db, 1, 2, 3);
        let mut session = db.voice_sessions_table.select(&id).await.unwrap().unwrap();

        session.channel_id = 4;
        db.voice_sessions_table.update(&session).await.unwrap();

        let fetched = db.voice_sessions_table.select(&id).await.unwrap().unwrap();
        assert_eq!(fetched.channel_id, 4);
    });

    db_test!(delete, |db| {
        let id = create_voice_session!(db, 1, 2, 3);
        db.voice_sessions_table.delete(&id).await.unwrap();
        assert!(db.voice_sessions_table.select(&id).await.unwrap().is_none());
    });

    db_test!(update_leave_time, |db| {
        let join_time = Utc::now();
        let session = VoiceSessionsModel {
            id: 0,
            user_id: 100,
            guild_id: 200,
            channel_id: 300,
            join_time,
            leave_time: join_time, // Active session
        };
        db.voice_sessions_table
            .insert(&session)
            .await
            .expect("Failed to insert session");

        // Update leave_time
        let new_leave_time = join_time + Duration::hours(1);
        db.voice_sessions_table
            .update_leave_time(100, 300, &join_time, &new_leave_time)
            .await
            .expect("Failed to update leave time");

        // Verify the update
        let sessions: Vec<VoiceSessionsModel> = db
            .voice_sessions_table
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
        let active_session = VoiceSessionsModel {
            id: 0,
            user_id: 100,
            guild_id: 200,
            channel_id: 300,
            join_time: now - Duration::hours(1),
            leave_time: now - Duration::hours(1), // Active
        };
        db.voice_sessions_table
            .insert(&active_session)
            .await
            .expect("Failed to insert active session");

        let completed_session = VoiceSessionsModel {
            id: 0,
            user_id: 101,
            guild_id: 200,
            channel_id: 301,
            join_time: now - Duration::hours(2),
            leave_time: now - Duration::hours(1), // Completed
        };
        db.voice_sessions_table
            .insert(&completed_session)
            .await
            .expect("Failed to insert completed session");

        let another_active = VoiceSessionsModel {
            id: 0,
            user_id: 102,
            guild_id: 200,
            channel_id: 302,
            join_time: now - Duration::minutes(30),
            leave_time: now - Duration::minutes(30), // Active
        };
        db.voice_sessions_table
            .insert(&another_active)
            .await
            .expect("Failed to insert another active session");

        // Find active sessions
        let active = db
            .voice_sessions_table
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
            .voice_sessions_table
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
            .voice_sessions_table
            .update_leave_time(999, 999, &join_time, &new_leave_time)
            .await;

        // Should not error, just not update anything
        assert!(result.is_ok());

        // Verify no sessions exist
        let sessions: Vec<VoiceSessionsModel> = db
            .voice_sessions_table
            .select_all()
            .await
            .expect("Failed to select sessions");
        assert!(sessions.is_empty());
    });
}
