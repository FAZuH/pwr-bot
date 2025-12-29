use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use pwr_bot::database::model::SubscriberType;
use pwr_bot::database::table::Table;
use pwr_bot::event::event_bus::EventBus;
use pwr_bot::event::feed_update_event::FeedUpdateEvent;
use pwr_bot::feed::feeds::Feeds;
use pwr_bot::feed::series_feed::SeriesItem;
use pwr_bot::feed::series_feed::SeriesLatest;
use pwr_bot::publisher::series_feed_publisher::SeriesFeedPublisher;
use pwr_bot::service::series_feed_subscription_service::SeriesFeedSubscriptionService;
use pwr_bot::service::series_feed_subscription_service::SubscribeResult;
use pwr_bot::service::series_feed_subscription_service::SubscriberTarget;
use tokio::time::sleep;

mod common;

#[tokio::test]
async fn test_subscription_and_publishing() {
    let (db, db_path) = common::setup_db().await;
    let event_bus = Arc::new(EventBus::new());

    // Setup Feeds
    let mut feeds = Feeds::new();
    let mock_domain = "mock.test";
    let mock_feed = Arc::new(common::MockFeed::new(mock_domain));
    feeds.add_feed(mock_feed.clone());
    let feeds = Arc::new(feeds);

    // Setup Service
    let service = Arc::new(SeriesFeedSubscriptionService {
        db: db.clone(),
        feeds: feeds.clone(),
    });

    // 1. Prepare Mock Data
    let series_id = "123";
    let series_url = format!("https://{}/title/{}", mock_domain, series_id);

    mock_feed.set_info(SeriesItem {
        id: series_id.to_string(),
        title: "Test Series".to_string(),
        url: series_url.clone(),
        description: "Desc".to_string(),
        cover_url: None,
    });

    let initial_latest = SeriesLatest {
        id: "ch1".to_string(),
        series_id: series_id.to_string(),
        latest: "Chapter 1".to_string(),
        url: format!("{}/chapter/1", series_url),
        published: Utc::now(),
    };
    mock_feed.set_latest(initial_latest.clone());

    // 2. Test Subscribe
    let target = SubscriberTarget {
        subscriber_type: SubscriberType::Dm,
        target_id: "user1".to_string(),
    };

    let subscriber = service
        .get_or_create_subscriber(&target)
        .await
        .expect("Failed to get or create subscriber");

    let result = service
        .subscribe(&series_url, &subscriber)
        .await
        .expect("Subscribe failed");
    match result {
        SubscribeResult::Success { feed } => {
            assert_eq!(feed.name, "Test Series");
            assert_eq!(feed.url, series_url);
        }
        _ => panic!("Expected Success"),
    }

    // Verify DB
    let subs = db.feed_subscription_table.select_all().await.unwrap();
    assert_eq!(subs.len(), 1);

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
        db.clone(),
        event_bus.clone(),
        feeds.clone(),
        Duration::from_millis(100), // Fast poll
    );
    publisher
        .clone()
        .start()
        .expect("Failed to start publisher");

    // Update Mock Data
    let new_latest = SeriesLatest {
        id: "ch2".to_string(),
        series_id: series_id.to_string(),
        latest: "Chapter 2".to_string(),
        url: format!("{}/chapter/2", series_url),
        published: Utc::now(),
    };
    mock_feed.set_latest(new_latest);

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
    let db_latest = db
        .feed_item_table
        .select_latest_by_feed_id(1)
        .await
        .unwrap();
    // Assuming feed ID is 1 because it's the first feed.

    assert_eq!(db_latest.description, "Chapter 2");

    // Cleanup
    publisher.stop().unwrap();
    common::teardown_db(db_path).await;
}
