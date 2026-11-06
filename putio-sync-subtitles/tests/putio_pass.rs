//! The Put.io pass for one video, against a fake source (no HTTP).

use std::collections::HashMap;
use std::sync::Mutex;

use putio_sync_client::{PutioSubtitle, SubtitleList};
use putio_sync_subtitles::engine::{fetch_for_video, EngineError, VideoRequest};
use putio_sync_subtitles::naming::SlotKind;
use putio_sync_subtitles::{BoxFut, SourceError, SubtitleSource};

const SRT: &str = "1\n00:00:01,000 --> 00:00:02,000\nHello\n\n";
const VTT: &str = "WEBVTT\n\n00:00:01.000 --> 00:00:02.000\nHello vtt\n";

struct Fake {
    list: Result<SubtitleList, SourceError>,
    bodies: HashMap<String, Result<Vec<u8>, SourceError>>,
    downloads: Mutex<Vec<String>>,
}

impl SubtitleSource for Fake {
    fn list<'a>(&'a self, _: i64, _: &'a [String]) -> BoxFut<'a, Result<SubtitleList, SourceError>> {
        let r = self.list.clone();
        Box::pin(async move { r })
    }
    fn download<'a>(
        &'a self,
        _: i64,
        s: &'a PutioSubtitle,
    ) -> BoxFut<'a, Result<Vec<u8>, SourceError>> {
        self.downloads.lock().unwrap().push(s.key.clone());
        let r = self
            .bodies
            .get(&s.key)
            .cloned()
            .unwrap_or(Err(SourceError::Failed("no body".into())));
        Box::pin(async move { r })
    }
}

fn sub(key: &str, code: &str, name: &str, source: &str) -> PutioSubtitle {
    PutioSubtitle {
        key: key.into(),
        format: "srt".into(),
        language: code.into(),
        language_code: code.into(),
        name: name.into(),
        source: source.into(),
        url: String::new(),
    }
}

fn fake(subs: Vec<PutioSubtitle>, bodies: Vec<(&str, Result<Vec<u8>, SourceError>)>) -> Fake {
    Fake {
        list: Ok(SubtitleList {
            default: None,
            subtitles: subs,
        }),
        bodies: bodies.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
        downloads: Mutex::new(Vec::new()),
    }
}

fn langs(l: &[&str]) -> Vec<String> {
    l.iter().map(|s| s.to_string()).collect()
}

fn video(dir: &tempfile::TempDir) -> std::path::PathBuf {
    let v = dir.path().join("Movie.2020.mkv");
    std::fs::write(&v, b"v").unwrap();
    v
}

#[tokio::test]
async fn en_is_saved_es_is_missing_and_fr_is_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let v = video(&dir);
    let f = fake(
        vec![
            sub("a", "en", "English", "opensubtitles"),
            sub("b", "fr", "French", "opensubtitles"),
        ],
        vec![("a", Ok(SRT.as_bytes().to_vec())), ("b", Ok(SRT.as_bytes().to_vec()))],
    );
    let wanted = langs(&["en", "es"]);
    let out = fetch_for_video(
        &f,
        &VideoRequest {
            video: &v,
            file_id: Some(9),
            languages: &wanted,
        },
    )
    .await
    .unwrap();
    assert!(dir.path().join("Movie.2020.en.srt").exists());
    assert!(!dir.path().join("Movie.2020.fr.srt").exists());
    assert_eq!(out.saved.len(), 1);
    assert_eq!(out.saved[0].source, "opensubtitles");
    assert_eq!(out.missing, ["es"]);
    assert_eq!(out.message(), "en: opensubtitles; es: missing");
    assert_eq!(*f.downloads.lock().unwrap(), ["a"], "fr was never downloaded");
}

#[tokio::test]
async fn folder_and_embedded_beat_opensubtitles() {
    let dir = tempfile::tempdir().unwrap();
    let v = video(&dir);
    let f = fake(
        vec![
            sub("os", "en", "English", "opensubtitles"),
            sub("emb", "en", "English", "mkv"),
        ],
        vec![
            ("os", Ok(SRT.as_bytes().to_vec())),
            ("emb", Ok(SRT.as_bytes().to_vec())),
        ],
    );
    let wanted = langs(&["en"]);
    let out = fetch_for_video(&f, &VideoRequest { video: &v, file_id: Some(1), languages: &wanted })
        .await
        .unwrap();
    assert_eq!(out.saved[0].source, "putio_embedded");
    assert_eq!(*f.downloads.lock().unwrap(), ["emb"]);
}

#[tokio::test]
async fn a_bad_candidate_falls_to_the_next() {
    let dir = tempfile::tempdir().unwrap();
    let v = video(&dir);
    let f = fake(
        vec![
            sub("bad", "en", "English", "mkv"),
            sub("good", "en", "English", "opensubtitles"),
        ],
        vec![
            ("bad", Ok(b"   \n".to_vec())),
            ("good", Ok(SRT.as_bytes().to_vec())),
        ],
    );
    let wanted = langs(&["en"]);
    let out = fetch_for_video(&f, &VideoRequest { video: &v, file_id: Some(1), languages: &wanted })
        .await
        .unwrap();
    assert_eq!(out.saved[0].source, "opensubtitles");
    assert_eq!(out.warnings.len(), 1);
    assert_eq!(*f.downloads.lock().unwrap(), ["bad", "good"]);
}

#[tokio::test]
async fn an_existing_sidecar_is_kept_and_not_fetched() {
    let dir = tempfile::tempdir().unwrap();
    let v = video(&dir);
    std::fs::write(dir.path().join("Movie.2020.en.srt"), "mine").unwrap();
    let f = fake(
        vec![sub("a", "en", "English", "opensubtitles")],
        vec![("a", Ok(SRT.as_bytes().to_vec()))],
    );
    let wanted = langs(&["en"]);
    let out = fetch_for_video(&f, &VideoRequest { video: &v, file_id: Some(1), languages: &wanted })
        .await
        .unwrap();
    assert!(out.saved.is_empty());
    assert_eq!(out.present, ["en"]);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("Movie.2020.en.srt")).unwrap(),
        "mine"
    );
    assert!(f.downloads.lock().unwrap().is_empty());
}

#[tokio::test]
async fn forced_and_sdh_go_to_their_own_names() {
    let dir = tempfile::tempdir().unwrap();
    let v = video(&dir);
    let f = fake(
        vec![
            sub("m", "en", "English", "opensubtitles"),
            sub("f", "en", "English forced", "opensubtitles"),
            sub("h", "en", "English SDH", "opensubtitles"),
        ],
        vec![
            ("m", Ok(SRT.as_bytes().to_vec())),
            ("f", Ok(SRT.as_bytes().to_vec())),
            ("h", Ok(SRT.as_bytes().to_vec())),
        ],
    );
    let wanted = langs(&["en"]);
    let out = fetch_for_video(&f, &VideoRequest { video: &v, file_id: Some(1), languages: &wanted })
        .await
        .unwrap();
    for n in ["Movie.2020.en.srt", "Movie.2020.en.forced.srt", "Movie.2020.en.sdh.srt"] {
        assert!(dir.path().join(n).exists(), "{n}");
    }
    let kinds: Vec<_> = out.saved.iter().map(|s| s.kind).collect();
    assert!(kinds.contains(&SlotKind::Forced) && kinds.contains(&SlotKind::Sdh));
}

#[tokio::test]
async fn forced_alone_does_not_fill_the_main_slot() {
    let dir = tempfile::tempdir().unwrap();
    let v = video(&dir);
    let f = fake(
        vec![sub("f", "en", "English forced", "opensubtitles")],
        vec![("f", Ok(SRT.as_bytes().to_vec()))],
    );
    let wanted = langs(&["en"]);
    let out = fetch_for_video(&f, &VideoRequest { video: &v, file_id: Some(1), languages: &wanted })
        .await
        .unwrap();
    assert!(!dir.path().join("Movie.2020.en.srt").exists());
    assert_eq!(out.missing, ["en"]);
}

#[tokio::test]
async fn vtt_and_latin1_are_saved_as_utf8_srt() {
    let dir = tempfile::tempdir().unwrap();
    let v = video(&dir);
    let latin1 = b"1\n00:00:01,000 --> 00:00:02,000\nCaf\xe9 \xf1and\xfa\n\n".to_vec();
    let f = fake(
        vec![
            sub("v", "en", "English", "mkv"),
            sub("l", "es", "Spanish", "opensubtitles"),
        ],
        vec![("v", Ok(VTT.as_bytes().to_vec())), ("l", Ok(latin1))],
    );
    let wanted = langs(&["en", "es"]);
    fetch_for_video(&f, &VideoRequest { video: &v, file_id: Some(1), languages: &wanted })
        .await
        .unwrap();
    let en = std::fs::read_to_string(dir.path().join("Movie.2020.en.srt")).unwrap();
    assert!(en.contains("00:00:01,000 --> 00:00:02,000") && en.contains("Hello vtt"));
    let es = std::fs::read_to_string(dir.path().join("Movie.2020.es.srt")).unwrap();
    assert!(es.contains("Café ñandú"), "{es}");
}

#[tokio::test]
async fn put_io_unavailable_is_not_an_error_but_a_failure_is() {
    let dir = tempfile::tempdir().unwrap();
    let v = video(&dir);
    let wanted = langs(&["en"]);
    let mut f = fake(vec![], vec![]);
    f.list = Err(SourceError::Unavailable("gone".into()));
    let out = fetch_for_video(&f, &VideoRequest { video: &v, file_id: Some(1), languages: &wanted })
        .await
        .unwrap();
    assert!(out.none_found());
    assert_eq!(out.missing, ["en"]);

    f.list = Err(SourceError::Failed("503".into()));
    let err = fetch_for_video(&f, &VideoRequest { video: &v, file_id: Some(1), languages: &wanted })
        .await
        .unwrap_err();
    assert_eq!(err, EngineError::Failed("503".into()));
}

#[tokio::test]
async fn a_download_that_fails_on_the_network_is_retried_by_the_step() {
    let dir = tempfile::tempdir().unwrap();
    let v = video(&dir);
    let f = fake(
        vec![sub("a", "en", "English", "mkv")],
        vec![("a", Err(SourceError::Failed("reset".into())))],
    );
    let wanted = langs(&["en"]);
    let err = fetch_for_video(&f, &VideoRequest { video: &v, file_id: Some(1), languages: &wanted })
        .await
        .unwrap_err();
    assert_eq!(err, EngineError::Failed("reset".into()));
}

#[tokio::test]
async fn no_file_id_means_no_put_io_calls() {
    let dir = tempfile::tempdir().unwrap();
    let v = video(&dir);
    let f = fake(vec![sub("a", "en", "English", "mkv")], vec![]);
    let wanted = langs(&["en"]);
    let out = fetch_for_video(&f, &VideoRequest { video: &v, file_id: None, languages: &wanted })
        .await
        .unwrap();
    assert_eq!(out.missing, ["en"]);
    assert!(f.downloads.lock().unwrap().is_empty());
}
