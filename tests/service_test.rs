use std::sync::Arc;

use chrono::Utc;
use pwr_bot::database::model::SubscriberType;
use pwr_bot::feed::feeds::Feeds;
use pwr_bot::feed::series_feed::SeriesItem;
use pwr_bot::feed::series_feed::SeriesLatest;
use pwr_bot::service::series_feed_subscription_service::SeriesFeedSubscriptionService;
use pwr_bot::service::series_feed_subscription_service::SubscriberTarget;

mod common;

#[tokio::test]
async fn test_get_or_create_subscriber() {
    let (db, db_path) = common::setup_db().await;
    let feeds = Arc::new(Feeds::new());
    let service = SeriesFeedSubscriptionService {
        db: db.clone(),
        feeds: feeds.clone(),
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
    let mut feeds = Feeds::new();
    let mock_domain = "test.com";
    let mock_feed = Arc::new(common::MockFeed::new(mock_domain));
    feeds.add_feed(mock_feed.clone());
    let feeds = Arc::new(feeds);

    let service = SeriesFeedSubscriptionService {
        db: db.clone(),
        feeds: feeds.clone(),
    };

    let series_id = "manga-1";
    let series_url = format!("https://{}/title/{}", mock_domain, series_id);

    mock_feed.set_info(SeriesItem {
        id: series_id.to_string(),
        title: "Test Manga".to_string(),
        url: series_url.clone(),
        description: "A test manga".to_string(),
        cover_url: None,
    });

    mock_feed.set_latest(SeriesLatest {
        id: "ch-1".to_string(),
        series_id: series_id.to_string(),
        latest: "Chapter 1".to_string(),
        url: format!("{}/chapter/1", series_url),
        published: Utc::now(),
    });

    // 1. Create new feed
    let feed1 = service
        .get_or_create_feed(&series_url)
        .await
        .expect("Failed to create feed");
    assert_eq!(feed1.name, "Test Manga");
    assert_eq!(feed1.url, series_url);
    assert!(feed1.id > 0);

    // 2. Get existing feed
    let feed2 = service
        .get_or_create_feed(&series_url)
        .await
        .expect("Failed to get feed");
    assert_eq!(feed1.id, feed2.id);
    assert_eq!(feed1.url, feed2.url);

    common::teardown_db(db_path).await;
}
