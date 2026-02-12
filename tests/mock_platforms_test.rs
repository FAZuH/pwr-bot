//! Tests for feed platform integrations using mock servers.

use std::path::PathBuf;

use httpmock::Method::GET;
use httpmock::Method::POST;
use httpmock::MockServer;
use pwr_bot::feed::Platform;
use pwr_bot::feed::anilist_platform::AniListPlatform;
use pwr_bot::feed::comick_platform::ComickPlatform;
use pwr_bot::feed::mangadex_platform::MangaDexPlatform;

/// Loads a test response file from the responses directory.
fn get_response(filename: &str) -> String {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("tests/responses");
    path.push(filename);
    std::fs::read_to_string(path).expect("Failed to read response file")
}

#[tokio::test]
async fn test_anilist_fetch_source() {
    let server = MockServer::start();
    let mut platform = AniListPlatform::new();
    platform.base.info.api_url = server.url("");

    let response_body = get_response("anilist_fetch_source_exist.json");
    let source_id = "101177";

    let mock = server.mock(|when, then| {
        when.method(POST).body_contains(source_id); // GraphQL query contains ID
        then.status(200)
            .header("content-type", "application/json")
            .body(response_body);
    });

    let source = platform
        .fetch_source(source_id)
        .await
        .expect("Failed to fetch source");

    mock.assert();
    assert_eq!(source.id, source_id);
    assert_eq!(
        source.name,
        "Chichi wa Eiyuu, Haha wa Seirei, Musume no Watashi wa Tenseisha."
    );
    assert!(
        source
            .image_url
            .unwrap()
            .contains("bx173692-shp7PGRQyCQl.jpg")
    );
}

#[tokio::test]
async fn test_anilist_fetch_latest() {
    let server = MockServer::start();
    let mut platform = AniListPlatform::new();
    platform.base.info.api_url = server.url("");

    let response_body = get_response("anilist_fetch_latest_exist.json");
    let items_id = "401043"; // Corresponds to ID in query or response

    let mock = server.mock(|when, then| {
        when.method(POST).body_contains(items_id);
        then.status(200)
            .header("content-type", "application/json")
            .body(response_body);
    });

    let item = platform
        .fetch_latest(items_id)
        .await
        .expect("Failed to fetch latest");

    mock.assert();
    assert_eq!(item.id, "401043");
    assert_eq!(item.title, "12"); // Episode number
    assert_eq!(item.published.timestamp(), 1766327400);
}

#[tokio::test]
async fn test_mangadex_fetch_source() {
    let server = MockServer::start();
    let mut platform = MangaDexPlatform::new();
    platform.base.info.api_url = server.url("");

    let response_body = get_response("mangadex_fetch_source_exist.json");
    let source_id = "0e017a08-835a-4cbe-ba63-576d5010a5a0";

    let mock = server.mock(|when, then| {
        when.method(GET).path(format!("/manga/{}", source_id));
        then.status(200)
            .header("content-type", "application/json")
            .body(response_body);
    });

    let source = platform
        .fetch_source(source_id)
        .await
        .expect("Failed to fetch source");

    mock.assert();
    assert_eq!(source.id, source_id);
    assert_eq!(source.name, "Kuma Kuma Kuma Bear");
    assert!(
        source
            .image_url
            .unwrap()
            .contains("7c198c70-6ab4-4e45-838b-f3efd9f5f1c1.jpg")
    );
}

#[tokio::test]
async fn test_mangadex_fetch_latest() {
    let server = MockServer::start();
    let mut platform = MangaDexPlatform::new();
    platform.base.info.api_url = server.url("");

    let response_body = get_response("mangadex_fetch_latest_exist.json");
    let items_id = "0e017a08-835a-4cbe-ba63-576d5010a5a0";

    let mock = server.mock(|when, then| {
        when.method(GET).path(format!("/manga/{}/feed", items_id));
        then.status(200)
            .header("content-type", "application/json")
            .body(response_body);
    });

    let item = platform
        .fetch_latest(items_id)
        .await
        .expect("Failed to fetch latest");

    mock.assert();
    assert_eq!(item.id, "eb39609e-2e48-4434-af76-aff0b7be91c2");
    assert_eq!(item.title, "105"); // Chapter number
    assert_eq!(item.published.to_rfc3339(), "2025-12-23T03:19:29+00:00");
}

#[tokio::test]
async fn test_comick_fetch_source() {
    let server = MockServer::start();
    let mut platform = ComickPlatform::new();
    platform.base.info.api_url = server.url("");

    let response_body = get_response("comick_fetch_source_exist.json");
    let slug = "02-tonikaku-kawaii";

    let mock = server.mock(|when, then| {
        when.method(GET).path(format!("/comic/{}", slug));
        then.status(200)
            .header("content-type", "application/json")
            .body(response_body);
    });

    let source = platform
        .fetch_source(slug)
        .await
        .expect("Failed to fetch source");

    mock.assert();
    assert_eq!(source.id, slug);
    assert_eq!(source.items_id, "DqrXZDbr"); // hid
    assert_eq!(source.name, "Tonikaku Kawaii");
    assert!(source.image_url.unwrap().contains("O8kwQg.jpg"));
}

#[tokio::test]
async fn test_comick_fetch_latest() {
    let server = MockServer::start();
    let mut platform = ComickPlatform::new();
    platform.base.info.api_url = server.url("");

    let response_body = get_response("comick_fetch_latest_exist.json");
    let hid = "DqrXZDbr";

    let mock = server.mock(|when, then| {
        when.method(GET).path(format!("/comic/{}/chapters", hid));
        then.status(200)
            .header("content-type", "application/json")
            .body(response_body);
    });

    let item = platform
        .fetch_latest(hid)
        .await
        .expect("Failed to fetch latest");

    mock.assert();
    assert_eq!(item.id, "DqrXZDbr");
    assert_eq!(item.title, "333"); // Chapter number
    // "2025-12-27T14:44:40.000Z"
    assert_eq!(item.published.timestamp(), 1766846680);
}
