//! Integration tests for feed subscription service.

use std::sync::Arc;

use chrono::Utc;
use feed::FeedItem;
use feed::FeedSource;
use feed::Platforms;
use feed::entity::FeedEntity;
use feed::entity::FeedItemEntity;
use feed::entity::SubscriberType;
use feed::repo::traits::*;
use feed::service::feed_subscription::FeedSubscriptionService;
use feed::service::feed_subscription::SubscriberTarget;
use pwr_plugin_protocol::FeedsSettings;

mod common;

#[serial_test::serial]
#[tokio::test]
async fn get_or_create_subscriber() {
    let db = common::setup_db().await;
    let feeds = Arc::new(Platforms::new());
    let service = FeedSubscriptionService::new(&db, feeds.clone());

    let target = SubscriberTarget {
        subscriber_type: SubscriberType::Dm,
        target_id: "user_123".to_string(),
    };

    // 1. Create new subscriber
    let sub1 = service
        .get_or_create_subscriber(&target)
        .await
        .expect("Failed to create subscriber");
    assert_eq!(sub1.target_id, "user_123");
    assert!(sub1.id > 0);

    // 2. Get existing subscriber
    let sub2 = service
        .get_or_create_subscriber(&target)
        .await
        .expect("Failed to get subscriber");
    assert_eq!(sub1.id, sub2.id);
    assert_eq!(sub1.target_id, sub2.target_id);

    common::teardown_db(&db).await;
}

#[serial_test::serial]
#[tokio::test]
async fn get_or_create_feed() {
    let db = common::setup_db().await;

    // Setup Mock Feed
    let mut feeds = Platforms::new();
    let mock_domain = "test.com";
    let mock_feed = Arc::new(common::MockFeed::new(mock_domain));
    feeds.add_platform(mock_feed.clone());
    let feeds = Arc::new(feeds);

    let service = FeedSubscriptionService::new(&db, feeds.clone());

    let source_id = "manga-1";
    let url = format!("https://{mock_domain}/title/{source_id}");

    mock_feed.set_info(FeedSource {
        id: source_id.to_string(),
        items_id: "abc".to_string(),
        name: "Test Manga".to_string(),
        source_url: url.clone(),
        description: "A test manga".to_string(),
        image_url: None,
    });

    mock_feed.set_latest(Some(FeedItem {
        id: "ch-1".to_string(),
        title: "Chapter 1".to_string(),
        published: Utc::now(),
    }));

    // 1. Create new feed
    let feed1 = service
        .get_or_create_feed(&url)
        .await
        .expect("Failed to create feed");
    assert_eq!(feed1.name, "Test Manga");
    assert_eq!(feed1.source_url, url);
    assert!(feed1.id > 0);

    // 2. Get existing feed
    let feed2 = service
        .get_or_create_feed(&url)
        .await
        .expect("Failed to get feed");
    assert_eq!(feed1.id, feed2.id);
    assert_eq!(feed1.source_url, feed2.source_url);

    // 3. Get feed with empty latest
    let source_id = "manga-2";
    let url = format!("https://{mock_domain}/title/{source_id}");
    mock_feed.set_info(FeedSource {
        id: source_id.to_string(),
        items_id: "abc".to_string(),
        name: "Test Manga 2".to_string(),
        description: "A test manga 2".to_string(),
        source_url: url.clone(),
        image_url: None,
    });
    mock_feed.set_latest(None);

    let feed3 = service
        .get_or_create_feed(&url)
        .await
        .expect("Failed to create feed");

    let feed4 = service
        .get_or_create_feed(&url)
        .await
        .expect("Failed to get feed");

    assert_eq!(feed3.id, feed4.id);
    assert_eq!(feed3.source_url, feed4.source_url);

    common::teardown_db(&db).await;
}

#[serial_test::serial]
#[tokio::test]
async fn feed_settings_service() {
    let db = common::setup_db().await;
    let feeds = Arc::new(Platforms::new());
    let service = FeedSubscriptionService::new(&db, feeds.clone());
    let guild_id: u64 = 1234567890;

    let settings = service
        .get_feed_settings(guild_id)
        .await
        .expect("get default feed settings");
    assert!(settings.channel_id.is_none());

    let updated = FeedsSettings {
        enabled: Some(false),
        channel_id: Some("chan_456".to_string()),
        subscribe_role_id: Some("role_123".to_string()),
        unsubscribe_role_id: Some("role_456".to_string()),
    };
    service
        .update_feed_settings(guild_id, updated.clone())
        .await
        .expect("update feed settings");

    let fetched = service
        .get_feed_settings(guild_id)
        .await
        .expect("get updated feed settings");
    assert_eq!(fetched, updated);

    common::teardown_db(&db).await;
}

#[serial_test::serial]
#[tokio::test]
async fn first_feed_settings_load_imports_the_legacy_server_settings_row() {
    use diesel::sql_types::BigInt;
    use diesel::sql_types::Jsonb;
    use diesel_async::RunQueryDsl;
    use pwr_plugin_protocol::ServerSettings;

    let db = common::setup_db().await;
    let service = FeedSubscriptionService::new(&db, Arc::new(Platforms::new()));
    let legacy = FeedsSettings {
        enabled: Some(false),
        channel_id: Some("legacy-channel".into()),
        subscribe_role_id: Some("legacy-sub-role".into()),
        unsubscribe_role_id: Some("legacy-unsub-role".into()),
    };
    let snapshot = serde_json::to_value(ServerSettings {
        feeds: legacy.clone(),
        ..ServerSettings::default()
    })
    .expect("serialize legacy settings");
    let mut connection = db.pool().get().await.expect("connect for legacy settings");
    diesel::sql_query("INSERT INTO server_settings (guild_id, settings) VALUES ($1, $2)")
        .bind::<BigInt, _>(42_i64)
        .bind::<Jsonb, _>(snapshot)
        .execute(&mut connection)
        .await
        .expect("insert legacy settings");

    let imported = service
        .get_feed_settings(42)
        .await
        .expect("import legacy feed settings");

    assert_eq!(imported, legacy);
    drop(connection);

    let changed_legacy = FeedsSettings {
        channel_id: Some("changed-after-import".into()),
        ..legacy.clone()
    };
    let changed_snapshot = serde_json::to_value(ServerSettings {
        feeds: changed_legacy,
        ..ServerSettings::default()
    })
    .expect("serialize changed legacy settings");
    let mut connection = db.pool().get().await.expect("connect for legacy update");
    diesel::sql_query("UPDATE server_settings SET settings = $2 WHERE guild_id = $1")
        .bind::<BigInt, _>(42_i64)
        .bind::<Jsonb, _>(changed_snapshot)
        .execute(&mut connection)
        .await
        .expect("change legacy settings after import");
    drop(connection);

    let cached = service
        .get_feed_settings(42)
        .await
        .expect("read imported feed settings");
    assert_eq!(cached, legacy);
    common::teardown_db(&db).await;
}

#[serial_test::serial]
#[tokio::test]
async fn list_paginated_subscriptions_optimization() {
    let db = common::setup_db().await;
    let feeds_platform = Arc::new(Platforms::new());

    let service = Arc::new(FeedSubscriptionService::new(&db, feeds_platform.clone()));

    // 1. Create Subscriber
    let target = SubscriberTarget {
        subscriber_type: SubscriberType::Dm,
        target_id: "user_paginated".to_string(),
    };
    let subscriber = service.get_or_create_subscriber(&target).await.unwrap();

    // 2. Create Feeds
    let feed_names = ["Zebra Feed", "Apple Feed", "Mango Feed", "Banana Feed"];

    for (i, name) in feed_names.iter().enumerate() {
        let feed = FeedEntity {
            name: name.to_string(),
            platform_id: "mock".to_string(),
            source_id: format!("src_{i}"),
            items_id: format!("items_{i}"),
            source_url: format!("http://mock/{i}/{name}"),
            ..Default::default()
        };
        let feed_id = db.feed.insert(&feed).await.unwrap();

        let sub_model = feed::entity::FeedSubscriptionEntity {
            feed_id,
            subscriber_id: subscriber.id,
            ..Default::default()
        };
        db.feed_subscription.insert(&sub_model).await.unwrap();

        // Add item for some feeds
        if i % 2 == 0 {
            let item = FeedItemEntity {
                feed_id,
                description: format!("Chapter {i}"),
                published: Utc::now(),
                ..Default::default()
            };
            db.feed_item.insert(&item).await.unwrap();
        }
    }

    // 3. Test Pagination & Sorting
    // Sorted names: Apple Feed, Banana Feed, Mango Feed, Zebra Feed
    // Page 1, Limit 2. Expected: Apple Feed, Banana Feed

    let result = service
        .list_paginated_subscriptions(&subscriber, 1u32, 2u32)
        .await
        .unwrap();

    assert_eq!(result.len(), 2);
    assert_eq!(result[0].feed.name, "Apple Feed");
    assert_eq!(result[1].feed.name, "Banana Feed");

    // Page 2, Limit 2. Expected: Mango Feed, Zebra Feed
    let result_p2 = service
        .list_paginated_subscriptions(&subscriber, 2u32, 2u32)
        .await
        .unwrap();

    assert_eq!(result_p2.len(), 2);
    assert_eq!(result_p2[0].feed.name, "Mango Feed");
    assert_eq!(result_p2[1].feed.name, "Zebra Feed");

    // Check item presence for Mango Feed (index 2) - has item
    assert!(result_p2[0].feed_latest.is_some());
    assert_eq!(
        result_p2[0].feed_latest.as_ref().unwrap().description,
        "Chapter 2"
    );

    common::teardown_db(&db).await;
}
