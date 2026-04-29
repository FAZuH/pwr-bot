//! Integration tests for publisher-subscriber event flow.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use pwr_bot::entity::SubscriberType;
use pwr_bot::event::FeedUpdateEvent;
use pwr_bot::event::event_bus::EventBus;
use pwr_bot::feed::FeedItem;
use pwr_bot::feed::FeedSource;
use pwr_bot::feed::Platforms;
use pwr_bot::repo::traits::*;
use pwr_bot::service::feed_subscription::FeedSubscriptionService;
use pwr_bot::service::feed_subscription::SubscribeResult;
use pwr_bot::service::feed_subscription::SubscriberTarget;
use pwr_bot::task::series_feed_publisher::SeriesFeedPublisher;
use tokio::time::sleep;

mod common;

#[tokio::test]
async fn test_subscription_and_publishing() {
    let db = common::setup_db().await;
    let event_bus = Arc::new(EventBus::new());

    // Setup Feeds
    let mut feeds = Platforms::new();
    let mock_domain = "mock.test";
    let mock_feed = Arc::new(common::MockFeed::new(mock_domain));
    feeds.add_platform(mock_feed.clone());
    let feeds = Arc::new(feeds);

    // Setup Service
    let service = Arc::new(FeedSubscriptionService::new(db.clone(), feeds.clone()));

    // 1. Prepare Mock Data
    let source_id = "123";
    let url = format!("https://{}/title/{}", mock_domain, source_id);

    mock_feed.set_info(FeedSource {
        id: source_id.to_string(),
        items_id: "abc".to_string(),
        name: "Test Name".to_string(),
        source_url: url.clone(),
        description: "Desc".to_string(),
        image_url: None,
    });

    let initial_latest = FeedItem {
        id: "ch1".to_string(),
        title: "Chapter 1".to_string(),
        published: Utc::now(),
    };
    mock_feed.set_latest(Some(initial_latest.clone()));

    // 2. Test Subscribe
    let target = SubscriberTarget {
        subscriber_type: SubscriberType::Dm,
        target_id: "user1".to_string(),
    };

    let subscriber = service
        .get_or_create_subscriber(&target)
        .await
        .expect("Failed to get or create subscriber");

    let feed_id = match service
        .subscribe(&url, &subscriber)
        .await
        .expect("Subscribe failed")
    {
        SubscribeResult::Success { feed } => {
            assert_eq!(feed.name, "Test Name");
            assert_eq!(feed.source_url, url);
            feed.id
        }
        _ => panic!("Expected Success"),
    };

    // Verify DB
    let subs = db.feed_subscription.select_all().await.unwrap();
    assert_eq!(subs.len(), 1);

    // Verify initial feed item exists
    let initial_items = db.feed_item.select_all_by_feed_id(feed_id).await.unwrap();
    assert!(!initial_items.is_empty(), "Initial feed item should exist");

    // 3. Test Publisher
    // Setup Event Listener
    let event_received = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let event_received_clone = event_received.clone();

    event_bus.register_callback(move |_event: FeedUpdateEvent| {
        event_received_clone.store(true, std::sync::atomic::Ordering::SeqCst);
        async { Ok(()) }
    });

    // Start Publisher
    let publisher = SeriesFeedPublisher::new(
        service.clone(),
        event_bus.clone(),
        Duration::from_millis(100), // Fast poll
    );
    publisher
        .clone()
        .start()
        .expect("Failed to start publisher");

    // Update Mock Data
    let new_latest = FeedItem {
        id: "ch2".to_string(),
        title: "Chapter 2".to_string(),
        published: Utc::now(),
    };
    mock_feed.set_latest(Some(new_latest));

    // Wait for poll
    let mut attempts = 0;
    while !event_received.load(std::sync::atomic::Ordering::SeqCst) && attempts < 50 {
        sleep(Duration::from_millis(100)).await;
        attempts += 1;
    }

    assert!(
        event_received.load(std::sync::atomic::Ordering::SeqCst),
        "Publisher did not fire event"
    );

    // Verify DB update
    let all_items = db.feed_item.select_all_by_feed_id(feed_id).await.unwrap();
    assert!(
        !all_items.is_empty(),
        "Feed items should exist after publisher update"
    );
    let db_latest = db
        .feed_item
        .select_latest_by_feed_id(feed_id)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(db_latest.description, "Chapter 2");

    // Cleanup
    publisher.stop().unwrap();
    common::teardown_db(&db).await;
}
