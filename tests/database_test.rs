mod common;

use chrono::Utc;
use pwr_bot::database::model::{FeedModel, FeedSubscriptionModel, FeedVersionModel, SubscriberModel, SubscriberType};
use pwr_bot::database::table::Table;

#[tokio::test]
async fn test_feed_table() {
    let db = common::get_in_memory_db().await;
    let feed_table = &db.feed_table;

    // Insert
    let feed = FeedModel {
        name: "Test Feed".to_string(),
        url: "https://example.com/feed".to_string(),
        tags: "test,feed".to_string(),
        ..Default::default()
    };
    let feed_id = feed_table.insert(&feed).await.unwrap();

    // Select
    let selected_feed = feed_table.select(&feed_id).await.unwrap();
    assert_eq!(selected_feed.name, feed.name);
    assert_eq!(selected_feed.url, feed.url);

    // Select by URL
    let selected_by_url = feed_table.select_by_url(&feed.url).await.unwrap();
    assert_eq!(selected_by_url.id, feed_id);

    // Update
    let updated_feed = FeedModel {
        id: feed_id,
        name: "Updated Feed".to_string(),
        url: feed.url.clone(),
        tags: feed.tags.clone(),
    };
    feed_table.update(&updated_feed).await.unwrap();
    let selected_after_update = feed_table.select(&feed_id).await.unwrap();
    assert_eq!(selected_after_update.name, "Updated Feed");

    // Delete
    feed_table.delete(&feed_id).await.unwrap();
    assert!(feed_table.select(&feed_id).await.is_err());

    // Select all
    let feed1_id = feed_table
        .insert(&FeedModel {
            name: "Feed 1".to_string(),
            url: "url1".to_string(),
            ..
            Default::default()
        })
        .await
        .unwrap();
    let feed2_id = feed_table
        .insert(&FeedModel {
            name: "Feed 2".to_string(),
            url: "url2".to_string(),
            ..
            Default::default()
        })
        .await
        .unwrap();
    let all_feeds = feed_table.select_all().await.unwrap();
    assert_eq!(all_feeds.len(), 2);
    assert!(all_feeds.iter().any(|f| f.id == feed1_id));
    assert!(all_feeds.iter().any(|f| f.id == feed2_id));
}

#[tokio::test]
async fn test_feed_version_table() {
    let db = common::get_in_memory_db().await;
    let feed_id = db.feed_table.insert(&FeedModel::default()).await.unwrap();
    let version_table = &db.feed_version_table;

    let version = FeedVersionModel {
        feed_id,
        version: "1.0".to_string(),
        published: Utc::now(),
        ..Default::default()
    };
    let version_id = version_table.insert(&version).await.unwrap();

    let selected = version_table.select(&version_id).await.unwrap();
    assert_eq!(selected.version, version.version);

    let latest = version_table.select_latest_by_feed_id(feed_id).await.unwrap();
    assert_eq!(latest.id, version_id);
}

#[tokio::test]
async fn test_subscriber_table() {
    let db = common::get_in_memory_db().await;
    let subscriber_table = &db.subscriber_table;

    let subscriber = SubscriberModel {
        r#type: SubscriberType::Dm,
        target_id: "12345".to_string(),
        ..Default::default()
    };
    let sub_id = subscriber_table.insert(&subscriber).await.unwrap();

    let selected = subscriber_table.select(&sub_id).await.unwrap();
    assert_eq!(selected.target_id, subscriber.target_id);

    let by_type_target = subscriber_table
        .select_by_type_and_target(subscriber.r#type, &subscriber.target_id)
        .await
        .unwrap();
    assert_eq!(by_type_target.id, sub_id);
}

#[tokio::test]
async fn test_feed_subscription_table() {
    let db = common::get_in_memory_db().await;
    let feed_id = db.feed_table.insert(&FeedModel::default()).await.unwrap();
    let subscriber_id = db.subscriber_table.insert(&SubscriberModel::default()).await.unwrap();
    let sub_table = &db.feed_subscription_table;

    let subscription = FeedSubscriptionModel {
        feed_id,
        subscriber_id,
        ..Default::default()
    };
    let subscription_id = sub_table.insert(&subscription).await.unwrap();

    let selected = sub_table.select(&subscription_id).await.unwrap();
    assert_eq!(selected.feed_id, feed_id);
    assert_eq!(selected.subscriber_id, subscriber_id);

    assert!(sub_table.exists(feed_id, subscriber_id).await.unwrap());

    let by_feed = sub_table.select_all_by_feed_id(feed_id).await.unwrap();
    assert_eq!(by_feed.len(), 1);

    let by_sub = sub_table.select_all_by_subscriber_id(subscriber_id).await.unwrap();
    assert_eq!(by_sub.len(), 1);

    sub_table.delete_subscription(feed_id, subscriber_id).await.unwrap();
    assert!(!sub_table.exists(feed_id, subscriber_id).await.unwrap());
}

#[tokio::test]
async fn test_cascade_delete() {
    let db = common::get_in_memory_db().await;
    let feed_id = db.feed_table.insert(&FeedModel::default()).await.unwrap();
    let sub_id = db.subscriber_table.insert(&SubscriberModel::default()).await.unwrap();

    let version_id = db
        .feed_version_table
        .insert(&FeedVersionModel {
            feed_id,
            ..Default::default()
        })
        .await
        .unwrap();

    let subscription_id = db
        .feed_subscription_table
        .insert(&FeedSubscriptionModel {
            feed_id,
            subscriber_id: sub_id,
            ..Default::default()
        })
        .await
        .unwrap();

    // Delete feed
    db.feed_table.delete(&feed_id).await.unwrap();

    // Check if associated versions and subscriptions are deleted
    assert!(db.feed_version_table.select(&version_id).await.is_err());
    assert!(db.feed_subscription_table.select(&subscription_id).await.is_err());
}