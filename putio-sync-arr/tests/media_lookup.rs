//! The ids the *arr history gives for a download (feature 050), against a mocked Radarr and
//! Sonarr. No real *arr is used.

use std::sync::Arc;

use mockito::{Matcher, ServerGuard};
use putio_sync_arr::{ArrApi, ArrMedia, ArrService};
use putio_sync_client::PutioApiClient;
use putio_sync_core::config::{ArrKind, ArrServer, ArrSettings};
use putio_sync_core::media::{MediaKind, MediaLookup, MediaRequest};
use putio_sync_db::{Database, NewArrItem};
use putio_sync_download::{DownloadConfig, DownloadManager};

const HASH: &str = "c12fe1c06bba254a9dc9f519b335aa7c1367a88a";

fn api(kind: ArrKind, url: &str) -> ArrApi {
    ArrApi::new(kind, url, "key", reqwest::Client::new())
}

fn json(server: &mut ServerGuard, query: Matcher, body: &str) -> mockito::Mock {
    server
        .mock("GET", "/api/v3/history")
        .match_query(query)
        .match_header("x-api-key", "key")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(body)
        .expect_at_least(0)
        .create()
}

#[tokio::test]
async fn radarr_history_gives_the_movie_ids() {
    let mut server = mockito::Server::new_async().await;
    let m = json(
        &mut server,
        Matcher::AllOf(vec![
            Matcher::UrlEncoded("downloadId".into(), HASH.to_uppercase()),
            Matcher::UrlEncoded("includeMovie".into(), "true".into()),
        ]),
        &format!(
            r#"{{"page":1,"records":[{{"downloadId":"{}","eventType":"grabbed",
            "movie":{{"title":"Breaking Bad: The Movie","year":2019,"imdbId":"tt0903747","tmdbId":559969}}}}]}}"#,
            HASH.to_uppercase()
        ),
    );
    let info = api(ArrKind::Radarr, &server.url())
        .history_by_download(HASH)
        .await
        .unwrap();
    assert_eq!(info.kind, Some(MediaKind::Movie));
    assert_eq!(info.imdb_id.as_deref(), Some("903747"));
    assert_eq!(info.tmdb_id, Some(559969));
    assert_eq!(info.year, Some(2019));
    assert!(info.episodes.is_empty());
    m.assert();
}

#[tokio::test]
async fn sonarr_history_collects_every_episode_of_a_season_pack() {
    let mut server = mockito::Server::new_async().await;
    let record = |e: u32| {
        format!(
            r#"{{"downloadId":"{}","series":{{"title":"Show","year":2008,"imdbId":"tt0903747","tmdbId":1396}},
            "episode":{{"seasonNumber":1,"episodeNumber":{e}}}}}"#,
            HASH.to_uppercase()
        )
    };
    // The same episode twice (grabbed, then imported) and a record of another download
    let body = format!(
        r#"{{"records":[{},{},{},{{"downloadId":"OTHER","series":{{"imdbId":"tt1"}},"episode":{{"seasonNumber":9,"episodeNumber":9}}}}]}}"#,
        record(2),
        record(1),
        record(2)
    );
    json(
        &mut server,
        Matcher::UrlEncoded("includeEpisode".into(), "true".into()),
        &body,
    );
    let info = api(ArrKind::Sonarr, &server.url())
        .history_by_download(HASH)
        .await
        .unwrap();
    assert_eq!(info.kind, Some(MediaKind::Episode));
    assert_eq!(info.episodes, [(1, 1), (1, 2)]);
    assert_eq!(info.tmdb_id, Some(1396));
}

#[tokio::test]
async fn an_unknown_download_gives_nothing() {
    let mut server = mockito::Server::new_async().await;
    json(&mut server, Matcher::Any, r#"{"records":[]}"#);
    let info = api(ArrKind::Radarr, &server.url())
        .history_by_download(HASH)
        .await
        .unwrap();
    assert_eq!(info.kind, None);
    assert!(!info.has_ids());
}

#[tokio::test]
async fn a_failing_arr_is_an_error() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("GET", "/api/v3/history")
        .match_query(Matcher::Any)
        .with_status(500)
        .create();
    assert!(api(ArrKind::Radarr, &server.url())
        .history_by_download(HASH)
        .await
        .is_err());
}

async fn adapter(arr_url: &str, dir: &tempfile::TempDir) -> ArrMedia {
    let db = Arc::new(Database::new(dir.path().join(".db")).await.unwrap());
    db.arr()
        .insert(&NewArrItem {
            hash: HASH,
            name: "Movie",
            category: "radarr",
            transfer_id: Some(1),
            putio_status: "COMPLETED",
            total_size: 10,
        })
        .await
        .unwrap();
    db.arr()
        .set_local(HASH, 500, "Movie", &[(501, 10)])
        .await
        .unwrap();
    let client = Arc::new(PutioApiClient::with_base_url(
        "token".to_string(),
        "http://127.0.0.1:9".to_string(),
    ));
    let downloads = Arc::new(DownloadManager::new(
        client.clone(),
        db.clone(),
        DownloadConfig {
            target_directory: dir.path().join("downloads"),
            ..DownloadConfig::default()
        },
    ));
    let settings = ArrSettings {
        enabled: true,
        username: "u".into(),
        password: Some("p".into()),
        servers: vec![ArrServer {
            kind: ArrKind::Radarr,
            url: arr_url.to_string(),
            api_key: Some("key".into()),
            name: None,
            category: Some("radarr".into()),
        }],
    };
    ArrMedia::new(Arc::new(ArrService::new(
        client,
        db,
        downloads,
        settings,
        dir.path().join("downloads"),
    )))
}

#[tokio::test]
async fn the_adapter_finds_the_item_of_a_file_and_remembers_the_answer() {
    let mut server = mockito::Server::new_async().await;
    let m = server
        .mock("GET", "/api/v3/history")
        .match_query(Matcher::Any)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(format!(
            r#"{{"records":[{{"downloadId":"{HASH}","movie":{{"title":"M","year":2020,"tmdbId":42}}}}]}}"#
        ))
        .expect(1)
        .create();
    let dir = tempfile::tempdir().unwrap();
    let media = adapter(&server.url(), &dir).await;
    // Both the folder and an accepted file map to the same item; the second call is cached
    for file_id in [500, 501] {
        let info = media
            .lookup(MediaRequest {
                putio_file_id: Some(file_id),
                category: Some("radarr"),
            })
            .await
            .unwrap();
        assert_eq!(info.tmdb_id, Some(42));
    }
    m.assert();
}

#[tokio::test]
async fn a_miss_is_remembered_too_and_an_unknown_file_gives_nothing() {
    let mut server = mockito::Server::new_async().await;
    let m = server
        .mock("GET", "/api/v3/history")
        .match_query(Matcher::Any)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"records":[]}"#)
        .expect(1)
        .create();
    let dir = tempfile::tempdir().unwrap();
    let media = adapter(&server.url(), &dir).await;
    let ask = |id| {
        media.lookup(MediaRequest {
            putio_file_id: Some(id),
            category: Some("radarr"),
        })
    };
    assert!(ask(500).await.is_none());
    assert!(ask(500).await.is_none());
    assert!(ask(999).await.is_none(), "not an *arr download");
    assert!(media.lookup(MediaRequest::default()).await.is_none());
    m.assert();
}
