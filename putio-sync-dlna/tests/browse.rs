//! Browse over SOAP against a real server on loopback (feature 022, US1).

mod common;
use common::Fixture;

#[tokio::test]
async fn root_lists_the_shared_roots_in_order() {
    let f = Fixture::start(&["cat:sonarr", "downloads", "cat:radarr"]).await;
    let reply = f.children("0").await;
    assert_eq!(reply.titles(), ["Downloads", "radarr", "sonarr"]);
    assert_eq!(reply.arg("TotalMatches"), "3");
    let ids: Vec<String> = reply.entries().into_iter().map(|e| e.1).collect();
    assert_eq!(ids, ["downloads/", "cat%3Aradarr/", "cat%3Asonarr/"]);

    let meta = f.browse("0", "BrowseMetadata", 0, 0).await;
    assert_eq!(meta.titles(), ["Test Server"]);
}

#[tokio::test]
async fn downloads_lists_folders_first_then_media_by_name() {
    let f = Fixture::start(&["downloads"]).await;
    f.write("b.mkv", b"x");
    f.write("A.mp4", b"x");
    f.write("Zdir/inner.mp4", b"x");
    f.write("adir/inner.mp4", b"x");
    f.write(".x.mkv.downloading", b"x");
    f.write(".hidden.mp4", b"x");
    f.write("a.nfo", b"x");
    f.write("b.torrent", b"x");
    f.write("song.flac", b"x");
    let reply = f.children("downloads/").await;
    assert_eq!(
        reply.titles(),
        ["adir", "Zdir", "A.mp4", "b.mkv", "song.flac"]
    );
    let kinds: Vec<String> = reply.entries().into_iter().map(|e| e.0).collect();
    assert_eq!(kinds, ["container", "container", "item", "item", "item"]);
    assert_eq!(reply.arg("NumberReturned"), "5");
    assert_eq!(reply.arg("TotalMatches"), "5");
}

#[tokio::test]
async fn subfolders_and_their_parent_ids() {
    let f = Fixture::start(&["downloads"]).await;
    f.write("Movies/Sub/film.mp4", b"x");
    let reply = f.children("downloads/Movies").await;
    assert_eq!(reply.titles(), ["Sub"]);
    assert!(reply.didl().contains("parentID=\"downloads/Movies\""));
    let inner = f.children("downloads/Movies/Sub").await;
    assert_eq!(inner.titles(), ["film.mp4"]);
    assert!(inner.didl().contains("parentID=\"downloads/Movies/Sub\""));
    let meta = f.browse("downloads/Movies", "BrowseMetadata", 0, 0).await;
    assert_eq!(meta.titles(), ["Movies"]);
    assert!(meta.didl().contains("parentID=\"downloads/\""));
}

#[tokio::test]
async fn paging_reports_the_full_total() {
    let f = Fixture::start(&["downloads"]).await;
    for i in 0..5 {
        f.write(&format!("f{i}.mp4"), b"x");
    }
    let page = f.browse("downloads/", "BrowseDirectChildren", 1, 2).await;
    assert_eq!(page.titles(), ["f1.mp4", "f2.mp4"]);
    assert_eq!(page.arg("NumberReturned"), "2");
    assert_eq!(page.arg("TotalMatches"), "5");
    let all = f.browse("downloads/", "BrowseDirectChildren", 0, 0).await;
    assert_eq!(all.arg("NumberReturned"), "5");
    let past = f.browse("downloads/", "BrowseDirectChildren", 9, 3).await;
    assert_eq!(past.arg("NumberReturned"), "0");
    assert_eq!(past.arg("TotalMatches"), "5");
}

#[tokio::test]
async fn paging_is_capped_at_500() {
    let f = Fixture::start(&["downloads"]).await;
    for i in 0..520 {
        f.write(&format!("f{i:04}.mp4"), b"");
    }
    let page = f.browse("downloads/", "BrowseDirectChildren", 0, 0).await;
    assert_eq!(page.arg("NumberReturned"), "500");
    assert_eq!(page.arg("TotalMatches"), "520");
    let page = f
        .browse("downloads/", "BrowseDirectChildren", 0, 9999)
        .await;
    assert_eq!(page.arg("NumberReturned"), "500");
}

#[tokio::test]
async fn metadata_of_an_item_has_size_class_and_url() {
    let f = Fixture::start(&["downloads"]).await;
    f.write("My Movie & Co.mkv", &[0u8; 1234]);
    let meta = f
        .browse(
            "downloads/My%20Movie%20%26%20Co.mkv",
            "BrowseMetadata",
            0,
            0,
        )
        .await;
    let didl = meta.didl();
    assert_eq!(meta.titles(), ["My Movie & Co.mkv"]);
    assert!(didl.contains("size=\"1234\""), "{didl}");
    assert!(didl.contains("object.item.videoItem"));
    assert!(didl.contains("video/x-matroska"));
    assert!(didl.contains(&format!(
        "http://127.0.0.1:{}/media/downloads/My%2520Movie%2520%2526%2520Co.mkv",
        f.port
    )));
}

#[tokio::test]
async fn a_new_file_shows_on_the_next_browse() {
    let f = Fixture::start(&["downloads"]).await;
    assert!(f.children("downloads/").await.titles().is_empty());
    f.write("new.mp4", b"x");
    assert_eq!(f.children("downloads/").await.titles(), ["new.mp4"]);
}

#[tokio::test]
async fn a_missing_shared_folder_is_an_empty_container() {
    let f = Fixture::start(&["downloads", "cat:nothing"]).await;
    let reply = f.children("cat%3Anothing/").await;
    assert!(reply.titles().is_empty());
    assert_eq!(reply.arg("TotalMatches"), "0");
}

#[tokio::test]
async fn other_actions_answer() {
    use putio_sync_dlna::soap;
    let f = Fixture::start(&["downloads"]).await;
    let id = common::soap_call(&f.base(), "/ctl/cds", soap::CDS, "GetSystemUpdateID", "").await;
    assert_eq!(id.status, 200);
    assert!(id.arg("Id").parse::<u32>().is_ok());
    let caps = common::soap_call(
        &f.base(),
        "/ctl/cds",
        soap::CDS,
        "GetSearchCapabilities",
        "",
    )
    .await;
    assert_eq!(caps.arg("SearchCaps"), "");
    let sort = common::soap_call(&f.base(), "/ctl/cds", soap::CDS, "GetSortCapabilities", "").await;
    assert_eq!(sort.arg("SortCaps"), "");
    let proto = common::soap_call(&f.base(), "/ctl/cms", soap::CMS, "GetProtocolInfo", "").await;
    assert!(proto.arg("Source").contains("http-get:*:video/mp4:"));
    assert_eq!(proto.arg("Sink"), "");
    let ids = common::soap_call(
        &f.base(),
        "/ctl/cms",
        soap::CMS,
        "GetCurrentConnectionIDs",
        "",
    )
    .await;
    assert_eq!(ids.arg("ConnectionIDs"), "0");
    let bad = common::soap_call(&f.base(), "/ctl/cds", soap::CDS, "Frobnicate", "").await;
    assert_eq!(bad.status, 500);
    assert_eq!(bad.fault(), Some(401));
}

#[tokio::test]
async fn descriptions_and_subscriptions() {
    let f = Fixture::start(&["downloads"]).await;
    let desc = f.get("/description.xml").await.text().await.unwrap();
    assert!(desc.contains("<friendlyName>Test Server</friendlyName>"));
    assert!(desc.contains(&format!("<UDN>uuid:{}</UDN>", f.server.uuid())));
    for path in ["/cds.xml", "/cms.xml"] {
        assert_eq!(f.get(path).await.status(), 200, "{path}");
    }
    assert_eq!(f.get("/nope").await.status(), 404);
    let sub = reqwest::Client::new()
        .request(
            reqwest::Method::from_bytes(b"SUBSCRIBE").unwrap(),
            format!("{}/evt/cds", f.base()),
        )
        .header("CALLBACK", "<http://127.0.0.1:1/>")
        .header("NT", "upnp:event")
        .send()
        .await
        .unwrap();
    assert_eq!(sub.status(), 200);
    assert!(sub.headers()["sid"].to_str().unwrap().starts_with("uuid:"));
    assert_eq!(sub.headers()["timeout"], "Second-1800");
    // Read-only: nothing else is accepted
    let put = reqwest::Client::new()
        .put(format!("{}/media/downloads/x.mp4", f.base()))
        .body("x")
        .send()
        .await
        .unwrap();
    assert_eq!(put.status(), 404);
}
