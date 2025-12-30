use chrono::Duration;
use chrono::Utc;
use pwr_bot::database::model::FeedItemModel;
use pwr_bot::database::model::FeedModel;
use pwr_bot::database::model::FeedSubscriptionModel;
use pwr_bot::database::model::ServerSettings;
use pwr_bot::database::model::ServerSettingsModel;
use pwr_bot::database::model::SubscriberModel;
use pwr_bot::database::model::SubscriberType;
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
                url: format!("https://{}.com", $name.replace(" ", "").to_lowercase()),
                ..Default::default()
            };
            $(feed.$field = $val.into();)* $db.feed_table.insert(&feed).await.expect("Failed to insert feed")
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

    db_test!(select_by_url, |db| {
        create_feed!(db, "Feed", { url: "https://unique.com" });
        let fetched = db
            .feed_table
            .select_by_url("https://unique.com")
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
