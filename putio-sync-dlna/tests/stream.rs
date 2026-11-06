//! Streaming local files over /media (feature 022, US1).

mod common;
use common::Fixture;
use futures_util::StreamExt;

fn pattern(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i % 251) as u8).collect()
}

#[tokio::test]
async fn full_get_equals_the_file_and_has_dlna_headers() {
    let f = Fixture::start(&["downloads"]).await;
    let bytes = pattern(1000);
    f.write("movie.mp4", &bytes);
    let r = f.get("/media/downloads/movie.mp4").await;
    assert_eq!(r.status(), 200);
    assert_eq!(r.headers()["accept-ranges"], "bytes");
    assert_eq!(r.headers()["content-type"], "video/mp4");
    assert_eq!(r.headers()["transfermode.dlna.org"], "Streaming");
    assert!(r.headers()["contentfeatures.dlna.org"]
        .to_str()
        .unwrap()
        .contains("DLNA.ORG_OP=01"));
    assert_eq!(r.bytes().await.unwrap().as_ref(), bytes.as_slice());
}

#[tokio::test]
async fn a_range_gives_206_with_those_bytes() {
    let f = Fixture::start(&["downloads"]).await;
    let bytes = pattern(1000);
    f.write("movie.mkv", &bytes);
    let r = reqwest::Client::new()
        .get(format!("{}/media/downloads/movie.mkv", f.base()))
        .header("Range", "bytes=100-199")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 206);
    assert_eq!(r.headers()["content-range"], "bytes 100-199/1000");
    assert_eq!(r.headers()["content-type"], "video/x-matroska");
    assert_eq!(r.bytes().await.unwrap().as_ref(), &bytes[100..200]);
}

#[tokio::test]
async fn a_range_past_the_end_is_416() {
    let f = Fixture::start(&["downloads"]).await;
    f.write("movie.mp4", &pattern(1000));
    let r = reqwest::Client::new()
        .get(format!("{}/media/downloads/movie.mp4", f.base()))
        .header("Range", "bytes=5000-6000")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 416);
    assert_eq!(r.headers()["content-range"], "bytes */1000");
}

#[tokio::test]
async fn head_has_the_headers_and_no_body() {
    let f = Fixture::start(&["downloads"]).await;
    f.write("movie.mp4", &pattern(1000));
    let r = reqwest::Client::new()
        .head(format!("{}/media/downloads/movie.mp4", f.base()))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(r.headers()["content-length"], "1000");
    assert_eq!(r.headers()["transfermode.dlna.org"], "Streaming");
    assert!(r.bytes().await.unwrap().is_empty());
}

#[tokio::test]
async fn the_url_from_browse_plays() {
    let f = Fixture::start(&["downloads"]).await;
    let bytes = pattern(300);
    f.write("Dir One/My Movie & Co (2020).mp4", &bytes);
    let meta = f
        .browse(
            "downloads/Dir%20One/My%20Movie%20%26%20Co%20%282020%29.mp4",
            "BrowseMetadata",
            0,
            0,
        )
        .await;
    let didl = meta.didl();
    let url = didl
        .split("</res>")
        .next()
        .unwrap()
        .rsplit('>')
        .next()
        .unwrap()
        .replace("&amp;", "&");
    let r = reqwest::get(&url).await.unwrap();
    assert_eq!(r.status(), 200, "{url}");
    assert_eq!(r.bytes().await.unwrap().as_ref(), bytes.as_slice());
}

#[tokio::test]
async fn a_file_cut_short_mid_stream_ends_the_body_with_an_error() {
    let f = Fixture::start(&["downloads"]).await;
    let path = f.write("big.mp4", b"x");
    let size: u64 = 96 * 1024 * 1024;
    std::fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(size)
        .unwrap();
    let r = f.get("/media/downloads/big.mp4").await;
    assert_eq!(r.status(), 200);
    let mut body = r.bytes_stream();
    let first = body.next().await.unwrap().unwrap();
    assert!(!first.is_empty());
    // The file is gone from under the stream
    std::fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(0)
        .unwrap();
    let mut total = first.len() as u64;
    let mut failed = false;
    while let Some(chunk) = body.next().await {
        match chunk {
            Ok(c) => total += c.len() as u64,
            Err(_) => {
                failed = true;
                break;
            }
        }
    }
    assert!(
        failed || total < size,
        "read {total} of {size} without an error"
    );
}

#[tokio::test]
async fn streams_active_counts_a_stream_in_flight() {
    let f = Fixture::start(&["downloads"]).await;
    let path = f.write("big.mp4", b"x");
    std::fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(64 * 1024 * 1024)
        .unwrap();
    assert_eq!(f.server.status().streams_active, 0);
    let r = f.get("/media/downloads/big.mp4").await;
    let mut body = r.bytes_stream();
    body.next().await.unwrap().unwrap();
    assert_eq!(f.server.status().streams_active, 1);
    drop(body);
    for _ in 0..50 {
        if f.server.status().streams_active == 0 {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("the stream never ended");
}
