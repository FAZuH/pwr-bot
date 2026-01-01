use std::sync::Arc;

use chrono::Utc;
use pwr_bot::database::model::SubscriberType;
use pwr_bot::feed::FeedItem;
use pwr_bot::feed::FeedSource;
use pwr_bot::feed::platforms::Platforms;
use pwr_bot::service::feed_subscription_service::FeedSubscriptionService;
use pwr_bot::service::feed_subscription_service::SubscriberTarget;

mod common;

#[tokio::test]
async fn test_get_or_create_subscriber() {
    let (db, db_path) = common::setup_db().await;
    let feeds = Arc::new(Platforms::new());
    let service = FeedSubscriptionService {
        db: db.clone(),
        platforms: feeds.clone(),
    };

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

    common::teardown_db(db_path).await;
}

#[tokio::test]
async fn test_get_or_create_feed() {
    let (db, db_path) = common::setup_db().await;

    // Setup Mock Feed
    let mut feeds = Platforms::new();
    let mock_domain = "test.com";
    let mock_feed = Arc::new(common::MockFeed::new(mock_domain));
    feeds.add_platform(mock_feed.clone());
    let feeds = Arc::new(feeds);

    let service = FeedSubscriptionService {
        db: db.clone(),
        platforms: feeds.clone(),
    };

    let source_id = "manga-1";
    let url = format!("https://{}/title/{}", mock_domain, source_id);

    mock_feed.set_info(FeedSource {
        id: source_id.to_string(),
        name: "Test Manga".to_string(),
        source_url: url.clone(),
        description: "A test manga".to_string(),
        image_url: None,
    });

    mock_feed.set_latest(Some(FeedItem {
        id: "ch-1".to_string(),
        source_id: source_id.to_string(),
        title: "Chapter 1".to_string(),
        item_url: format!("{}/chapter/1", url),
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
    let url = format!("https://{}/title/{}", mock_domain, source_id);
    mock_feed.set_info(FeedSource {
        id: source_id.to_string(),
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

    common::teardown_db(db_path).await;
}

#[tokio::test]
async fn test_server_settings_service() {
    let (db, db_path) = common::setup_db().await;
    let feeds = Arc::new(Platforms::new());
    let service = FeedSubscriptionService {
        db: db.clone(),
        platforms: feeds.clone(),
    };

    use pwr_bot::database::model::ServerSettings;

    let guild_id: u64 = 1234567890;

    // 1. Get default settings
    let settings = service
        .get_server_settings(guild_id)
        .await
        .expect("Failed to get settings");
    assert!(settings.channel_id.is_none());

    // 2. Update settings
    let new_settings = ServerSettings {
        enabled: Some(false),
        channel_id: Some("chan_456".to_string()),
        subscribe_role_id: Some("role_123".to_string()),
        unsubscribe_role_id: Some("role_456".to_string()),
    };
    service
        .update_server_settings(guild_id, new_settings.clone())
        .await
        .expect("Failed to update");

    // 3. Get updated settings
    let fetched = service
        .get_server_settings(guild_id)
        .await
        .expect("Failed to get updated settings");
    assert_eq!(fetched.channel_id, Some("chan_456".to_string()));
    assert_eq!(fetched.subscribe_role_id, Some("role_123".to_string()));
    assert_eq!(fetched.unsubscribe_role_id, Some("role_456".to_string()));

    common::teardown_db(db_path).await;
}
