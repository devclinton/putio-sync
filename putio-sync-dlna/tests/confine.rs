//! Nothing outside a shared root is reachable (feature 022, US1/US2).
//! Single-threaded runtime so the log capture sees every event.

mod common;
use common::{capture_logs, raw_get, Fixture};

#[tokio::test(flavor = "current_thread")]
async fn browse_refuses_traversal_ids() {
    let (_g, logs) = capture_logs();
    let f = Fixture::start(&["downloads"]).await;
    f.write("ok.mp4", b"x");
    for id in [
        "downloads/..",
        "downloads/%2E%2E",
        "downloads/%2e%2e/%2e%2e",
        "downloads//etc",
        "downloads/%2Fetc",
        "downloads/a/../../..",
        "downloads/.hidden",
        "nothing/",
        "cat%3Aunshared/",
    ] {
        let r = f.browse(id, "BrowseDirectChildren", 0, 0).await;
        assert_eq!(r.status, 500, "{id}");
        assert_eq!(r.fault(), Some(701), "{id}");
    }
    assert!(
        logs.text().contains("DLNA browse refused"),
        "{}",
        logs.text()
    );
}

#[tokio::test(flavor = "current_thread")]
async fn media_refuses_traversal_paths() {
    let (_g, logs) = capture_logs();
    let f = Fixture::start(&["downloads"]).await;
    f.write("ok.mp4", b"x");
    for path in [
        "/media/downloads/../../etc/passwd",
        "/media/downloads/%2E%2E/%2E%2E/etc/passwd",
        "/media/downloads/%252E%252E/%252E%252E/etc/passwd",
        "/media/downloads/%2e%2e%2f%2e%2e%2fetc%2fpasswd",
        "/media/downloads//etc/passwd",
        "/media/downloads/%252Fetc%252Fpasswd",
        "/media/other/ok.mp4",
    ] {
        let (status, body) = raw_get(f.port, path).await;
        assert!(status == 404 || status == 400, "{path} gave {status}");
        assert!(!String::from_utf8_lossy(&body).contains("root:"), "{path}");
    }
    // The legitimate path still works
    let (status, _) = raw_get(f.port, "/media/downloads/ok.mp4").await;
    assert_eq!(status, 200);
    assert!(
        logs.text().contains("DLNA stream refused"),
        "{}",
        logs.text()
    );
}

#[cfg(unix)]
#[tokio::test]
async fn symlinks_out_of_the_root_are_hidden_and_refused() {
    use std::os::unix::fs::symlink;
    let f = Fixture::start(&["downloads"]).await;
    let secret = tempfile::tempdir().unwrap();
    std::fs::write(secret.path().join("secret.mp4"), b"top secret").unwrap();
    symlink(
        secret.path().join("secret.mp4"),
        f.downloads.path().join("link.mp4"),
    )
    .unwrap();
    symlink(secret.path(), f.downloads.path().join("linkdir")).unwrap();
    f.write("fine.mp4", b"fine");
    // Inside the root a link is fine
    symlink(
        f.downloads.path().join("fine.mp4"),
        f.downloads.path().join("inside.mp4"),
    )
    .unwrap();

    let titles = f.children("downloads/").await.titles();
    assert_eq!(titles, ["fine.mp4", "inside.mp4"]);

    let (status, body) = raw_get(f.port, "/media/downloads/link.mp4").await;
    assert_eq!(status, 404);
    assert!(!String::from_utf8_lossy(&body).contains("top secret"));
    let (status, _) = raw_get(f.port, "/media/downloads/linkdir/secret.mp4").await;
    assert_eq!(status, 404);
    let r = f.children("downloads/linkdir").await;
    assert_eq!(r.fault(), Some(701));
    let (status, body) = raw_get(f.port, "/media/downloads/inside.mp4").await;
    assert_eq!(status, 200);
    assert_eq!(body, b"fine");
}

#[tokio::test]
async fn a_category_root_cannot_reach_its_siblings() {
    let f = Fixture::start(&["cat:tv"]).await;
    f.write("tv/show.mp4", b"show");
    f.write("movies/film.mp4", b"film");
    f.write("loose.mp4", b"loose");
    let ids = f.children("cat%3Atv/").await.titles();
    assert_eq!(ids, ["show.mp4"]);
    for path in [
        "/media/cat%253Atv/..%2Fmovies%2Ffilm.mp4",
        "/media/cat%253Atv/%252E%252E/movies/film.mp4",
        "/media/downloads/loose.mp4",
        "/media/downloads/movies/film.mp4",
    ] {
        let (status, _) = raw_get(f.port, path).await;
        assert_eq!(status, 404, "{path}");
    }
    let (status, body) = raw_get(f.port, "/media/cat%253Atv/show.mp4").await;
    assert_eq!(status, 200);
    assert_eq!(body, b"show");
    assert_eq!(f.children("downloads/").await.fault(), Some(701));
}

#[tokio::test]
async fn hidden_partial_and_non_media_files_are_not_served() {
    let f = Fixture::start(&["downloads"]).await;
    f.write(".secret.mp4", b"x");
    f.write("movie.mp4.downloading", b"x");
    f.write(".movie.mp4.downloading", b"x");
    f.write("notes.txt", b"x");
    f.write("a.torrent", b"x");
    f.write(".dir/inner.mp4", b"x");
    for path in [
        "/media/downloads/.secret.mp4",
        "/media/downloads/movie.mp4.downloading",
        "/media/downloads/.movie.mp4.downloading",
        "/media/downloads/notes.txt",
        "/media/downloads/a.torrent",
        "/media/downloads/.dir/inner.mp4",
    ] {
        let (status, _) = raw_get(f.port, path).await;
        assert_eq!(status, 404, "{path}");
    }
}

#[tokio::test]
async fn a_root_unshared_by_an_update_is_gone_at_once() {
    let mut f = Fixture::start(&["downloads", "cat:tv"]).await;
    f.write("tv/show.mp4", b"show");
    f.write("a.mp4", b"a");
    let (status, _) = raw_get(f.port, "/media/cat%253Atv/show.mp4").await;
    assert_eq!(status, 200);

    f.config.dlna.roots = vec!["downloads".into()];
    f.server.update(&f.config).await;
    let (status, _) = raw_get(f.port, "/media/cat%253Atv/show.mp4").await;
    assert_eq!(status, 404);
    assert_eq!(f.children("cat%3Atv/").await.fault(), Some(701));
    assert_eq!(f.children("0").await.titles(), ["Downloads"]);
    let (status, _) = raw_get(f.port, "/media/downloads/a.mp4").await;
    assert_eq!(status, 200);
}
