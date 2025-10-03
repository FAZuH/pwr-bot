use httpmock::prelude::*;
use pwr_bot::source::anilist_source::AniListSource;
use pwr_bot::source::mangadex_source::MangaDexSource;
use pwr_bot::source::Source;

#[tokio::test]
async fn test_anilist_get_latest_returns_anime_on_valid_response() {
    let server = MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(POST);
        then.status(200).json_body(serde_json::json!({
            "data": {
                "Media": {
                    "title": { "romaji": "Test Anime" },
                    "nextAiringEpisode": {
                        "airingAt": 1234567890,
                        "episode": 5
                    }
                }
            }
        }));
    });

    let mut source = AniListSource::new();
    let server_url = server.url("");
    source.base.url.api_url = &server_url;

    let result = source.get_latest("123").await.unwrap();
    if let pwr_bot::source::model::SourceResult::Series(anime) = result {
        assert_eq!(anime.id, "123");
        assert_eq!(anime.title, "Test Anime");
        assert_eq!(anime.latest, "5");
        assert_eq!(anime.url, "https://anilist.co/anime/123");
    } else {
        panic!("Wrong result type");
    }
    mock.assert();
}

#[tokio::test]
async fn test_anilist_get_latest_returns_error_when_no_next_airing_episode() {
    let server = MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(POST);
        then.status(200).json_body(serde_json::json!({
            "data": {
                "Media": {
                    "title": { "romaji": "Test Anime" },
                    "nextAiringEpisode": null
                }
            }
        }));
    });

    let mut source = AniListSource::new();
    let server_url = server.url("");
    source.base.url.api_url = &server_url;

    let result = source.get_latest("456").await;
    assert!(result.is_err());
    mock.assert();
}

#[test]
fn test_anilist_get_id_from_url() {
    let source = AniListSource::new();
    let url = "https://anilist.co/anime/123/test";
    let id = source.get_id_from_url(url).unwrap();
    assert_eq!(id, "123");
}

#[tokio::test]
async fn test_mangadex_get_latest_returns_manga_on_valid_response() {
    let server = MockServer::start();
    let series_id = "a1b2c3d4-e5f6-a7b8-c9d0-e1f2a3b4c5d6";

    let title_mock = server.mock(|when, then| {
        when.method(GET).path(format!("/manga/{}", series_id));
        then.status(200).json_body(serde_json::json!({
            "data": {
                "attributes": {
                    "title": {
                        "en": "Test Manga"
                    }
                }
            }
        }));
    });

    let feed_mock = server.mock(|when, then| {
        when.method(GET).path(format!("/manga/{}/feed", series_id));
        then.status(200).json_body(serde_json::json!({
            "data": [
                {
                    "attributes": {
                        "chapter": "10",
                        "publishAt": "2025-01-01T00:00:00+00:00"
                    }
                }
            ]
        }));
    });

    let mut source = MangaDexSource::new();
    let server_url = server.url("");
    source.base.url.api_url = &server_url;

    let result = source.get_latest(series_id).await.unwrap();
    if let pwr_bot::source::model::SourceResult::Series(manga) = result {
        assert_eq!(manga.id, series_id);
        assert_eq!(manga.title, "Test Manga");
        assert_eq!(manga.latest, "10");
    } else {
        panic!("Wrong result type");
    }

    title_mock.assert();
    feed_mock.assert();
}

#[test]
fn test_mangadex_get_id_from_url() {
    let source = MangaDexSource::new();
    let url = "https://mangadex.org/title/a1b2c3d4-e5f6-a7b8-c9d0-e1f2a3b4c5d6/test";
    let id = source.get_id_from_url(url).unwrap();
    assert_eq!(id, "a1b2c3d4-e5f6-a7b8-c9d0-e1f2a3b4c5d6");
}