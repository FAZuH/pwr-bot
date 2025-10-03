use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures::lock::Mutex;
use httpmock::{Mock, prelude::*};
use pwr_bot::database::model::{FeedModel, FeedSubscriptionModel, FeedVersionModel, SubscriberModel, SubscriberType};
use pwr_bot::database::table::Table;
use serde_json::json;
use tokio::time::sleep;

use pwr_bot::database::database::Database;
use pwr_bot::event::event_bus::EventBus;
use pwr_bot::event::feed_update_event::FeedUpdateEvent;
use pwr_bot::publisher::feed_publisher::FeedPublisher;
use pwr_bot::source::anilist_source::AniListSource;
use pwr_bot::source::mangadex_source::MangaDexSource;
use pwr_bot::source::sources::Sources;
use pwr_bot::subscriber::Subscriber;

mod common;

#[derive(Clone)]
struct MockFeedSubscriber {
    pub received_events: Arc<Mutex<Vec<FeedUpdateEvent>>>,
}

impl MockFeedSubscriber {
    fn new() -> Self {
        Self {
            received_events: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

#[async_trait]
impl Subscriber<FeedUpdateEvent> for MockFeedSubscriber {
    async fn callback(&self, event: FeedUpdateEvent) -> anyhow::Result<()> {
        self.received_events.lock().await.push(event);
        Ok(())
    }
}

async fn wait_for_request(mock: &Mock<'_>, threshhold: usize) {
    while mock.hits() < threshhold {
        sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test]
async fn test_feed_publisher_and_subscriber() -> anyhow::Result<()> {
    // 0:Setup
    let bus = Arc::new(EventBus::new());
    let db = common::get_in_memory_db().await;
    let server = MockServer::start();
    let sources = Arc::new(Sources::new());
    let subscriber = Arc::new(MockFeedSubscriber::new());
    bus.register_subcriber(subscriber.clone());

    let publisher = FeedPublisher::new(
        db.clone(),
        bus.clone(),
        sources.clone(),
        Duration::from_secs(1),
    );

    // Populate db
    let feed = FeedModel {
        name: "Test Manga".to_string(),
        url: "https://mangadex.org/title/789".to_string(),
        tags: "series".to_string(),
        ..Default::default()
    };
    let feed_id = db.feed_table.insert(&feed).await?;
    db.feed_version_table.insert(&FeedVersionModel {
        feed_id,
        version: "41".to_string(),
        ..Default::default()
    }).await?;
    let subscriber_id = db.subscriber_table.insert(&SubscriberModel {
        r#type: SubscriberType::Dm,
        target_id: "123".to_string(),
        ..Default::default()
    }).await?;
    db.feed_subscription_table.insert(&FeedSubscriptionModel {
        feed_id,
        subscriber_id,
        ..Default::default()
    }).await?;

    // 1:Setup:Mock initial API response
    let mut mock = server.mock(|when, then| {
        when.method(GET).path("/title/789");
        then.status(200)
            .header("content-type", "application/json")
            .json_body(json!({
                "data": {
                    "attributes": {
                        "title": {
                            "en": "Test Manga"
                        }
                    }
                }
            }));
    });
    let mut feed_mock = server.mock(|when, then| {
        when.method(GET).path("/title/789/feed");
        then.status(200)
            .header("content-type", "application/json")
            .json_body(json!({
                "data": [{
                    "id": "999",
                    "attributes": { "chapter": "42", "publishAt": "2025-07-14T02:35:03+00:00" }
                }]
            }));
    });

    // 1:Act:Start publisher and wait for it to run
    publisher.clone().start()?;
    sleep(Duration::from_secs(2)).await;
    publisher.clone().stop()?;

    // 1:Assert:Verify manga update event
    assert_eq!(subscriber.received_events.lock().await.len(), 1);

    Ok(())
}