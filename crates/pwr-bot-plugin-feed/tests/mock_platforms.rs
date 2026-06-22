use std::path::PathBuf;

use httpmock::Method::GET;
use httpmock::Method::POST;
use httpmock::MockServer;
use pwr_bot_plugin_feed::platform::AniListPlatform;
use pwr_bot_plugin_feed::platform::ComickPlatform;
use pwr_bot_plugin_feed::platform::MangaDexPlatform;
use pwr_bot_plugin_feed::platform::Platform;

fn load_fixture(name: &str) -> String {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("tests/fixtures");
    path.push(name);
    std::fs::read_to_string(path).unwrap_or_else(|_| panic!("missing fixture: {name}"))
}

// ---- AniList ----

#[tokio::test]
async fn anilist_fetch_source() {
    let server = MockServer::start();
    let mut platform = AniListPlatform::new();
    platform.base.info.api_url = server.url("");

    let body = load_fixture("anilist_fetch_source_exist.json");
    let mock = server.mock(|when, then| {
        when.method(POST).body_contains("173692");
        then.status(200)
            .header("content-type", "application/json")
            .body(body);
    });

    let source = platform.fetch_source("173692").await.unwrap();

    mock.assert();
    assert_eq!(source.id, "173692");
    assert_eq!(
        source.name,
        "Chichi wa Eiyuu, Haha wa Seirei, Musume no Watashi wa Tenseisha."
    );
    assert!(source.image_url.unwrap().contains("bx173692"));
}

#[tokio::test]
async fn anilist_fetch_latest() {
    let server = MockServer::start();
    let mut platform = AniListPlatform::new();
    platform.base.info.api_url = server.url("");

    let body = load_fixture("anilist_fetch_latest_exist.json");
    let mock = server.mock(|when, then| {
        when.method(POST).body_contains("173692");
        then.status(200)
            .header("content-type", "application/json")
            .body(body);
    });

    let item = platform.fetch_latest("173692").await.unwrap();

    mock.assert();
    assert_eq!(item.id, "401043");
    assert_eq!(item.title, "12");
    assert_eq!(item.published.timestamp(), 1766327400);
}

#[tokio::test]
async fn anilist_fetch_latest_null_schedule_returns_item_not_found() {
    let server = MockServer::start();
    let mut platform = AniListPlatform::new();
    platform.base.info.api_url = server.url("");

    let body = load_fixture("anilist_fetch_latest_not_exist.json");
    let mock = server.mock(|when, then| {
        when.method(POST).body_contains("999999");
        then.status(200)
            .header("content-type", "application/json")
            .body(body);
    });

    let result = platform.fetch_latest("999999").await;

    mock.assert();
    assert!(result.is_err());
}

#[tokio::test]
async fn anilist_fetch_source_not_found_returns_error() {
    let server = MockServer::start();
    let mut platform = AniListPlatform::new();
    platform.base.info.api_url = server.url("");

    let body = load_fixture("anilist_fetch_source_not_exist.json");
    let mock = server.mock(|when, then| {
        when.method(POST).body_contains("999999");
        then.status(200)
            .header("content-type", "application/json")
            .body(body);
    });

    let result = platform.fetch_source("999999").await;

    mock.assert();
    assert!(result.is_err());
}

// ---- MangaDex ----

#[tokio::test]
async fn mangadex_fetch_source() {
    let server = MockServer::start();
    let mut platform = MangaDexPlatform::new();
    platform.base.info.api_url = server.url("");

    let id = "0e017a08-835a-4cbe-ba63-576d5010a5a0";
    let body = load_fixture("mangadex_fetch_source_exist.json");
    let mock = server.mock(|when, then| {
        when.method(GET).path(format!("/manga/{id}"));
        then.status(200)
            .header("content-type", "application/json")
            .body(body);
    });

    let source = platform.fetch_source(id).await.unwrap();

    mock.assert();
    assert_eq!(source.id, id);
    assert_eq!(source.name, "Kuma Kuma Kuma Bear");
    assert!(source.image_url.unwrap().contains("7c198c70"));
}

#[tokio::test]
async fn mangadex_fetch_latest() {
    let server = MockServer::start();
    let mut platform = MangaDexPlatform::new();
    platform.base.info.api_url = server.url("");

    let id = "0e017a08-835a-4cbe-ba63-576d5010a5a0";
    let body = load_fixture("mangadex_fetch_latest_exist.json");
    let mock = server.mock(|when, then| {
        when.method(GET).path(format!("/manga/{id}/feed"));
        then.status(200)
            .header("content-type", "application/json")
            .body(body);
    });

    let item = platform.fetch_latest(id).await.unwrap();

    mock.assert();
    assert_eq!(item.id, "eb39609e-2e48-4434-af76-aff0b7be91c2");
    assert_eq!(item.title, "105");
    assert_eq!(item.published.to_rfc3339(), "2025-12-23T03:19:29+00:00");
}

#[tokio::test]
async fn mangadex_fetch_latest_empty_data_returns_empty_source() {
    let server = MockServer::start();
    let mut platform = MangaDexPlatform::new();
    platform.base.info.api_url = server.url("");

    let id = "0e017a08-835a-4cbe-ba63-576d5010a5a0";
    let body = load_fixture("mangadex_fetch_latest_not_exist.json");
    let mock = server.mock(|when, then| {
        when.method(GET).path(format!("/manga/{id}/feed"));
        then.status(200)
            .header("content-type", "application/json")
            .body(body);
    });

    let result = platform.fetch_latest(id).await;

    mock.assert();
    assert!(result.is_err());
}

#[tokio::test]
async fn mangadex_fetch_source_not_found_returns_error() {
    let server = MockServer::start();
    let mut platform = MangaDexPlatform::new();
    platform.base.info.api_url = server.url("");

    let id = "00000000-0000-0000-0000-000000000000";
    let body = load_fixture("mangadex_fetch_source_not_exist.json");
    let mock = server.mock(|when, then| {
        when.method(GET).path(format!("/manga/{id}"));
        then.status(200)
            .header("content-type", "application/json")
            .body(body);
    });

    let result = platform.fetch_source(id).await;

    mock.assert();
    assert!(result.is_err());
}

// ---- Comick ----

#[tokio::test]
async fn comick_fetch_source() {
    let server = MockServer::start();
    let mut platform = ComickPlatform::new();
    platform.base.info.api_url = server.url("");

    let slug = "02-tonikaku-kawaii";
    let body = load_fixture("comick_fetch_source_exist.json");
    let mock = server.mock(|when, then| {
        when.method(GET).path(format!("/comic/{slug}"));
        then.status(200)
            .header("content-type", "application/json")
            .body(body);
    });

    let source = platform.fetch_source(slug).await.unwrap();

    mock.assert();
    assert_eq!(source.id, slug);
    assert_eq!(source.items_id, "DqrXZDbr");
    assert_eq!(source.name, "Tonikaku Kawaii");
    assert!(source.image_url.unwrap().contains("O8kwQg.jpg"));
}

#[tokio::test]
async fn comick_fetch_latest() {
    let server = MockServer::start();
    let mut platform = ComickPlatform::new();
    platform.base.info.api_url = server.url("");

    let hid = "DqrXZDbr";
    let body = load_fixture("comick_fetch_latest_exist.json");
    let mock = server.mock(|when, then| {
        when.method(GET).path(format!("/comic/{hid}/chapters"));
        then.status(200)
            .header("content-type", "application/json")
            .body(body);
    });

    let item = platform.fetch_latest(hid).await.unwrap();

    mock.assert();
    assert_eq!(item.id, "DqrXZDbr");
    assert_eq!(item.title, "333");
    assert_eq!(item.published.timestamp(), 1766846680);
}

#[tokio::test]
async fn comick_fetch_latest_not_found_returns_error() {
    let server = MockServer::start();
    let mut platform = ComickPlatform::new();
    platform.base.info.api_url = server.url("");

    let hid = "nonexistent";
    let body = load_fixture("comick_fetch_latest_not_exist.json");
    let mock = server.mock(|when, then| {
        when.method(GET).path(format!("/comic/{hid}/chapters"));
        then.status(200)
            .header("content-type", "application/json")
            .body(body);
    });

    let result = platform.fetch_latest(hid).await;

    mock.assert();
    assert!(result.is_err());
}
