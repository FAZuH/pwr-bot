use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures::lock::Mutex;
use httpmock::Mock;
use httpmock::prelude::*;
use pwr_bot::database::database::Database;
use pwr_bot::database::model::latest_results_model::LatestResultModel;
use pwr_bot::database::model::subscribers_model::SubscribersModel;
use pwr_bot::database::table::table::Table;
use pwr_bot::event::event_bus::EventBus;
use pwr_bot::event::manga_update_event::MangaUpdateEvent;
use pwr_bot::event::series_update_event::SeriesUpdateEvent;
use pwr_bot::feed::anilist_series_feed::AniListSeriesFeed;
use pwr_bot::feed::mangadex_series_feed::MangaDexSeriesFeed;
use pwr_bot::publisher::anime_update_publisher::AnimeUpdatePublisher;
use pwr_bot::publisher::manga_update_publisher::MangaUpdatePublisher;
use pwr_bot::subscriber::subscriber::Subscriber;
use serde_json::json;
use tokio::time::sleep;

#[derive(Clone)]
struct MockMangaSubscriber {
    pub received_events: Arc<Mutex<Vec<MangaUpdateEvent>>>,
}
impl MockMangaSubscriber {
    fn new() -> Self {
        Self {
            received_events: Arc::new(Mutex::new(Vec::new())),
        }
    }
}
#[async_trait]
impl Subscriber<MangaUpdateEvent> for MockMangaSubscriber {
    async fn callback(&self, event: MangaUpdateEvent) -> anyhow::Result<()> {
        self.received_events.lock().await.push(event);
        Ok(())
    }
}

#[derive(Clone)]
struct MockAnimeSubscriber {
    pub received_events: Arc<Mutex<Vec<SeriesUpdateEvent>>>,
}
impl MockAnimeSubscriber {
    fn new() -> Self {
        Self {
            received_events: Arc::new(Mutex::new(Vec::new())),
        }
    }
}
#[async_trait]
impl Subscriber<SeriesUpdateEvent> for MockAnimeSubscriber {
    async fn callback(&self, event: SeriesUpdateEvent) -> anyhow::Result<()> {
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
async fn test_manga_publisher_and_subscriber() -> anyhow::Result<()> {
    // 0:Setup
    let bus = Arc::new(EventBus::new());
    let db = Arc::new(Database::new("sqlite://test.db", "test.db").await?);
    db.drop_all_tables().await?;
    db.create_all_tables().await?;
    let server = MockServer::start();
    let source = Arc::new(MangaDexSeriesFeed::new_with_url(server.url("")));
    // 0:Setup:Subscriber
    let subscriber = Arc::new(MockMangaSubscriber::new());
    bus.register_subcriber(subscriber.clone()).await;
    // 0:Setup:Publisher
    let publisher = MangaUpdatePublisher::new(
        db.clone(),
        bus.clone(),
        source.clone(),
        Duration::from_secs(u64::MAX),
    );
    // 0:Setup:Populate db
    let series_id = "789";
    let latest_update_id = db
        .latest_results_table
        .insert(&LatestResultModel {
            url: series_id.to_string(),
            r#type: "manga".to_string(),
            latest: "41".to_string(),
            ..Default::default()
        })
        .await?;
    db.subscribers_table
        .insert(&SubscribersModel {
            latest_update_id,
            subscriber_type: "manga".to_string(),
            ..Default::default()
        })
        .await?;

    // 1:Setup:Mock initial API response
    let mut mock = server.mock(|when, then| {
        when.method(GET).path(format!("/manga/{}/feed", series_id));
        then.status(200)
            .header("content-type", "application/json")
            .json_body(json!({
                "data": [{
                    "id": "999",  // New series latest
                    "attributes": { "chapter": "42", "publishAt": "2025-07-14T02:35:03+00:00" }
                }]
            }));
    });

    // 1:Act:Start publisher and wait for it to run
    publisher.clone().start()?;
    wait_for_request(&mock, 1).await;
    publisher.clone().stop()?;

    // 1:Assert:Verify manga update event
    mock.assert();
    assert_eq!(subscriber.received_events.lock().await.len(), 1);

    // 2:Setup
    subscriber.received_events.lock().await.clear();

    // 2:Act:Run again with same data
    publisher.clone().start()?;
    wait_for_request(&mock, 2).await;
    publisher.clone().stop()?;

    // 2:Assert:No new event should be published
    assert_eq!(subscriber.received_events.lock().await.len(), 0);

    // 3:Setup:Mock updated API response
    mock.delete();
    let mock_updated = server.mock(|when, then| {
        when.method(GET).path(format!("/manga/{}/feed", "789"));
        then.status(200)
            .header("content-type", "application/json")
            .json_body(json!({
                "data": [{
                    "id": "1000", // Newer series latest
                    "attributes": { "chapter": "43", "publishAt": "2025-07-15T02:35:03+00:00" }
                }]
            }));
    });

    // 3:Act:Run again with new data
    publisher.clone().start()?;
    wait_for_request(&mock_updated, 1).await;
    publisher.clone().stop()?;

    // 3:Assert:Verify new event
    mock_updated.assert();
    assert_eq!(subscriber.received_events.lock().await.len(), 1);

    // 0:Teardown
    db.drop_all_tables().await?;
    Ok(())
}

#[tokio::test]
async fn test_anime_publisher_and_subscriber() -> anyhow::Result<()> {
    // 0:Setup
    let bus = Arc::new(EventBus::new());
    let db = Arc::new(Database::new("sqlite://test.db", "test.db").await?);
    db.drop_all_tables().await?;
    db.create_all_tables().await?;
    let server = MockServer::start();
    let source = Arc::new(AniListSeriesFeed::new_with_url(server.url("")));
    // 0:Setup:Subscriber
    let subscriber = Arc::new(MockAnimeSubscriber::new());
    bus.register_subcriber(subscriber.clone()).await;
    // 0:Setup:Publisher
    let publisher = AnimeUpdatePublisher::new(
        db.clone(),
        bus.clone(),
        source.clone(),
        Duration::from_secs(u64::MAX),
    );
    // 0:Setup:Populate db
    let series_id = "456";
    let latest_update_id = db
        .latest_results_table
        .insert(&LatestResultModel {
            url: series_id.to_string(),
            r#type: "anime".to_string(),
            latest: "4".to_string(),
            ..Default::default()
        })
        .await?;
    db.subscribers_table
        .insert(&SubscribersModel {
            latest_update_id,
            subscriber_type: "anime".to_string(),
            ..Default::default()
        })
        .await?;

    // 1:Setup:Mock initial API response
    let mut mock = server.mock(|when, then| {
        when.method(POST).path("/");
        then.status(200)
            .header("content-type", "application/json")
            .json_body(json!({
                "data": {
                    "Media": {
                        "title": { "romaji": "Test Anime" },
                        "nextAiringEpisode": {
                            "airingAt": 1721779200,
                            "episode": 5
                        }
                    }
                }
            }));
    });

    // 1:Act:Start publisher and wait for it to run
    publisher.clone().start()?;
    wait_for_request(&mock, 1).await;
    publisher.clone().stop()?;

    // 1:Assert:Verify anime update event
    mock.assert();
    assert_eq!(subscriber.received_events.lock().await.len(), 1);

    // 2:Setup
    subscriber.received_events.lock().await.clear();

    // 2:Act:Run again with same data
    publisher.clone().start()?;
    wait_for_request(&mock, 2).await;
    publisher.clone().stop()?;

    // 2:Assert:No new event should be published
    assert_eq!(subscriber.received_events.lock().await.len(), 0);

    // 3:Setup:Mock updated API response
    mock.delete();
    let mock_updated = server.mock(|when, then| {
        when.method(POST).path("/");
        then.status(200)
            .header("content-type", "application/json")
            .json_body(json!({
                "data": {
                    "Media": {
                        "title": { "romaji": "Test Anime" },
                        "nextAiringEpisode": {
                            "airingAt": 1721865600,
                            "episode": 6
                        }
                    }
                }
            }));
    });

    // 3:Act:Run again with new data
    publisher.clone().start()?;
    wait_for_request(&mock_updated, 1).await;
    publisher.clone().stop()?;

    // 3:Assert:Verify new event
    mock_updated.assert();
    assert_eq!(subscriber.received_events.lock().await.len(), 1);

    // 0:Teardown
    db.drop_all_tables().await?;
    Ok(())
}
