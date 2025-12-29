use chrono::Utc;
use pwr_bot::database::model::FeedItemModel;
use pwr_bot::database::model::FeedModel;
use pwr_bot::database::model::FeedSubscriptionModel;
use pwr_bot::database::model::SubscriberModel;
use pwr_bot::database::model::SubscriberType;
use pwr_bot::database::table::Table;

mod common;

#[tokio::test]
async fn test_feed_table_crud() {
    let (db, db_path) = common::setup_db().await;
    let table = &db.feed_table;

    // 1. Create (Insert)
    let feed = FeedModel {
        name: "Test Feed".to_string(),
        url: "https://test.com".to_string(),
        description: "Test Description".to_string(),
        ..Default::default()
    };
    let id = table.insert(&feed).await.expect("Failed to insert feed");
    assert!(id > 0);

    // 2. Read (Select)
    let fetched = table.select(&id).await.expect("Failed to select feed");
    assert_eq!(fetched.name, feed.name);
    assert_eq!(fetched.url, feed.url);

    // 3. Update
    let mut updated_feed = fetched.clone();
    updated_feed.name = "Updated Feed".to_string();
    table
        .update(&updated_feed)
        .await
        .expect("Failed to update feed");

    let fetched_updated = table
        .select(&id)
        .await
        .expect("Failed to select updated feed");
    assert_eq!(fetched_updated.name, "Updated Feed");

    // 4. Replace
    let mut replaced_feed = fetched_updated.clone();
    replaced_feed.description = "Replaced Description".to_string();
    let replace_id = table
        .replace(&replaced_feed)
        .await
        .expect("Failed to replace");
    // SQLite REPLACE increments ID on conflict if using AUTOINCREMENT
    assert!(replace_id >= id);

    let fetched_replaced = table
        .select(&replace_id)
        .await
        .expect("Failed to select replaced");
    assert_eq!(fetched_replaced.description, "Replaced Description");

    // 5. Delete
    table
        .delete(&replace_id)
        .await
        .expect("Failed to delete feed");
    let result = table.select(&replace_id).await;
    assert!(result.is_err());

    common::teardown_db(db_path).await;
}

#[tokio::test]
async fn test_feed_table_custom_methods() {
    let (db, db_path) = common::setup_db().await;
    let table = &db.feed_table;

    let feed1 = FeedModel {
        name: "Feed 1".to_string(),
        url: "https://site1.com".to_string(),
        tags: "manga,shonen".to_string(),
        ..Default::default()
    };
    let feed2 = FeedModel {
        name: "Feed 2".to_string(),
        url: "https://site2.com".to_string(),
        tags: "anime,shonen".to_string(),
        ..Default::default()
    };

    table.insert(&feed1).await.unwrap();
    table.insert(&feed2).await.unwrap();

    // select_by_url
    let f1 = table.select_by_url("https://site1.com").await.unwrap();
    assert_eq!(f1.name, "Feed 1");

    // select_all_by_tag
    let shonen = table.select_all_by_tag("shonen").await.unwrap();
    assert_eq!(shonen.len(), 2);
    let anime = table.select_all_by_tag("anime").await.unwrap();
    assert_eq!(anime.len(), 1);
    assert_eq!(anime[0].name, "Feed 2");

    common::teardown_db(db_path).await;
}

#[tokio::test]
async fn test_feed_item_table_operations() {
    let (db, db_path) = common::setup_db().await;
    let feed_table = &db.feed_table;
    let item_table = &db.feed_item_table;

    // Setup feed
    let feed = FeedModel {
        name: "Feed".to_string(),
        url: "http://test.com".to_string(),
        ..Default::default()
    };
    let feed_id = feed_table.insert(&feed).await.unwrap();

    // 1. Insert
    let item1 = FeedItemModel {
        feed_id,
        description: "Chapter 1".to_string(),
        published: Utc::now(),
        ..Default::default()
    };
    let _id1 = item_table.insert(&item1).await.unwrap();

    let item2 = FeedItemModel {
        feed_id,
        description: "Chapter 2".to_string(),
        published: Utc::now() + chrono::Duration::hours(1),
        ..Default::default()
    };
    let _id2 = item_table.insert(&item2).await.unwrap();

    // 2. Select Latest
    let latest = item_table.select_latest_by_feed_id(feed_id).await.unwrap();
    assert_eq!(latest.description, "Chapter 2");

    // 3. Select All by Feed ID
    let all = item_table.select_all_by_feed_id(feed_id).await.unwrap();
    assert_eq!(all.len(), 2);
    // Ordered by published DESC
    assert_eq!(all[0].description, "Chapter 2");
    assert_eq!(all[1].description, "Chapter 1");

    // 4. Update
    let mut updated_item = latest.clone();
    updated_item.description = "Chapter 2 Updated".to_string();
    item_table.update(&updated_item).await.unwrap();
    let fetched = item_table.select(&updated_item.id).await.unwrap();
    assert_eq!(fetched.description, "Chapter 2 Updated");

    // 5. Delete All by Feed ID
    item_table.delete_all_by_feed_id(feed_id).await.unwrap();
    let all_after = item_table.select_all_by_feed_id(feed_id).await.unwrap();
    assert!(all_after.is_empty());

    common::teardown_db(db_path).await;
}

#[tokio::test]
async fn test_subscriber_table_operations() {
    let (db, db_path) = common::setup_db().await;
    let table = &db.subscriber_table;

    // 1. Insert
    let sub1 = SubscriberModel {
        r#type: SubscriberType::Dm,
        target_id: "user1".to_string(),
        ..Default::default()
    };
    let id1 = table.insert(&sub1).await.unwrap();

    let sub2 = SubscriberModel {
        r#type: SubscriberType::Guild,
        target_id: "guild1".to_string(),
        ..Default::default()
    };
    let _id2 = table.insert(&sub2).await.unwrap();

    // 2. Select by Type and Target
    let fetched = table
        .select_by_type_and_target(&SubscriberType::Dm, "user1")
        .await
        .unwrap();
    assert_eq!(fetched.id, id1);

    // 3. Select All by Type and Feed (Need FeedSubscription for this)
    // Setup Feed and Subscription
    let feed_id = db
        .feed_table
        .insert(&FeedModel {
            name: "F".to_string(),
            url: "u".to_string(),
            ..Default::default()
        })
        .await
        .unwrap();

    db.feed_subscription_table
        .insert(&FeedSubscriptionModel {
            feed_id,
            subscriber_id: id1,
            ..Default::default()
        })
        .await
        .unwrap();

    let subs = table
        .select_all_by_type_and_feed(SubscriberType::Dm, feed_id)
        .await
        .unwrap();
    assert_eq!(subs.len(), 1);
    assert_eq!(subs[0].target_id, "user1");

    common::teardown_db(db_path).await;
}

#[tokio::test]
async fn test_feed_subscription_table_operations() {
    let (db, db_path) = common::setup_db().await;
    let sub_table = &db.subscriber_table;
    let feed_table = &db.feed_table;
    let fs_table = &db.feed_subscription_table;

    // Setup
    let feed_id = feed_table
        .insert(&FeedModel {
            name: "Feed".to_string(),
            url: "url".to_string(),
            ..Default::default()
        })
        .await
        .unwrap();

    let sub_id = sub_table
        .insert(&SubscriberModel {
            r#type: SubscriberType::Dm,
            target_id: "u1".to_string(),
            ..Default::default()
        })
        .await
        .unwrap();

    // 1. Insert & Exists
    let fs = FeedSubscriptionModel {
        feed_id,
        subscriber_id: sub_id,
        ..Default::default()
    };
    fs_table.insert(&fs).await.unwrap();

    assert!(fs_table.exists_by_feed_id(feed_id).await.unwrap());

    // 2. Count
    let count = fs_table.count_by_subscriber_id(sub_id).await.unwrap();
    assert_eq!(count, 1);

    // 3. Select All by Feed/Subscriber
    let by_feed = fs_table.select_all_by_feed_id(feed_id).await.unwrap();
    assert_eq!(by_feed.len(), 1);

    let by_sub = fs_table.select_all_by_subscriber_id(sub_id).await.unwrap();
    assert_eq!(by_sub.len(), 1);

    // 4. Paginated
    let paginated = fs_table
        .select_paginated_by_subscriber_id(sub_id, 0u32, 10u32)
        .await
        .unwrap();
    assert_eq!(paginated.len(), 1);

    // 5. Delete Subscription
    fs_table.delete_subscription(feed_id, sub_id).await.unwrap();
    assert!(!fs_table.exists_by_feed_id(feed_id).await.unwrap());

    // 6. Delete All by ...
    // Re-insert
    fs_table.insert(&fs).await.unwrap();
    fs_table.delete_all_by_subscriber_id(sub_id).await.unwrap();
    assert_eq!(fs_table.count_by_subscriber_id(sub_id).await.unwrap(), 0);

    // Re-insert
    fs_table.insert(&fs).await.unwrap();
    fs_table.delete_all_by_feed_id(feed_id).await.unwrap();
    assert!(!fs_table.exists_by_feed_id(feed_id).await.unwrap());

    common::teardown_db(db_path).await;
}

#[tokio::test]
async fn test_feed_table_select_by_name_and_subscriber_id() {
    let (db, db_path) = common::setup_db().await;
    let feed_table = &db.feed_table;
    let sub_table = &db.subscriber_table;
    let sub_feed_table = &db.feed_subscription_table;

    // 1. Setup Subscriber
    let sub = SubscriberModel {
        r#type: SubscriberType::Dm,
        target_id: "user1".to_string(),
        ..Default::default()
    };
    let sub_id = sub_table.insert(&sub).await.unwrap();

    // 2. Setup Feeds
    let feed1 = FeedModel {
        name: "One Piece".to_string(),
        url: "url1".to_string(),
        ..Default::default()
    };
    let feed1_id = feed_table.insert(&feed1).await.unwrap();

    let feed2 = FeedModel {
        name: "One Punch Man".to_string(),
        url: "url2".to_string(),
        ..Default::default()
    };
    let feed2_id = feed_table.insert(&feed2).await.unwrap();

    let feed3 = FeedModel {
        name: "Naruto".to_string(),
        url: "url3".to_string(),
        ..Default::default()
    };
    let _feed3_id = feed_table.insert(&feed3).await.unwrap();

    // 3. Setup Subscriptions (Sub1 -> Feed1, Feed2)
    sub_feed_table
        .insert(&FeedSubscriptionModel {
            feed_id: feed1_id,
            subscriber_id: sub_id,
            ..Default::default()
        })
        .await
        .unwrap();

    sub_feed_table
        .insert(&FeedSubscriptionModel {
            feed_id: feed2_id,
            subscriber_id: sub_id,
            ..Default::default()
        })
        .await
        .unwrap();

    // 4. Test Search "one" -> Expects 2 (case insensitive)
    let results = feed_table
        .select_by_name_and_subscriber_id(&sub_id, "one", None)
        .await
        .unwrap();
    assert_eq!(results.len(), 2);
    // Sort or check content. Order is not guaranteed by SQL without ORDER BY, but usually insertion order for small sets.
    // Let's just check existence.
    let names: Vec<String> = results.iter().map(|f| f.name.clone()).collect();
    assert!(names.contains(&"One Piece".to_string()));
    assert!(names.contains(&"One Punch Man".to_string()));

    // 5. Test Search "Naruto" -> Expects 0 (not subscribed)
    let results = feed_table
        .select_by_name_and_subscriber_id(&sub_id, "Naruto", None)
        .await
        .unwrap();
    assert_eq!(results.len(), 0);

    // 6. Test Limit
    let results = feed_table
        .select_by_name_and_subscriber_id(&sub_id, "One", 1u32)
        .await
        .unwrap();
    assert_eq!(results.len(), 1);

    common::teardown_db(db_path).await;
}

#[tokio::test]
async fn test_server_settings_table_crud() {
    let (db, db_path) = common::setup_db().await;
    let table = &db.server_settings_table;

    use pwr_bot::database::model::ServerSettings;
    use pwr_bot::database::model::ServerSettingsModel;

    // 1. Insert (using replace actually, or insert)
    let settings = ServerSettings {
        channel_id: Some("123".to_string()),
    };
    let model = ServerSettingsModel {
        guild_id: 1234567890,
        settings: sqlx::types::Json(settings),
    };

    let id = table.insert(&model).await.expect("Failed to insert settings");
    assert_eq!(id, 1234567890);

    // 2. Select
    let fetched = table
        .select(&1234567890)
        .await
        .expect("Failed to select");
    assert_eq!(fetched.settings.0.channel_id, Some("123".to_string()));

    // 3. Update
    let mut new_settings = fetched.settings.0.clone();
    new_settings.channel_id = Some("456".to_string());
    let updated_model = ServerSettingsModel {
        guild_id: 1234567890,
        settings: sqlx::types::Json(new_settings),
    };
    table
        .update(&updated_model)
        .await
        .expect("Failed to update");

    let fetched2 = table
        .select(&1234567890)
        .await
        .expect("Failed to select updated");
    assert_eq!(fetched2.settings.0.channel_id, Some("456".to_string()));

    // 4. Delete
    table
        .delete(&1234567890)
        .await
        .expect("Failed to delete");
    assert!(table.select(&1234567890).await.is_err());

    common::teardown_db(db_path).await;
}
