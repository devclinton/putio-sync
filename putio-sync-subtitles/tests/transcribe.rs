//! The Whisper client against a mock server (feature 025). No real server is contacted.

use mockito::Matcher;
use putio_sync_core::subtitles::TranscriptionConfig;
use putio_sync_subtitles::transcribe::{TranscribeError, Transcriber};

fn config(url: &str) -> TranscriptionConfig {
    TranscriptionConfig {
        enabled: true,
        url: url.to_string(),
        ..Default::default()
    }
}

const GOOD: &str = r#"{"language":"spanish","duration":5.0,"text":"Hola","segments":[{"start":0.5,"end":2.0,"text":" Hola"}]}"#;

#[tokio::test]
async fn the_request_has_the_contract_fields_and_the_key_as_bearer() {
    let mut server = mockito::Server::new_async().await;
    let m = server
        .mock("POST", "/v1/audio/transcriptions")
        .match_header("authorization", "Bearer sekrit")
        .match_header("content-type", Matcher::Regex("multipart/form-data".into()))
        .match_body(Matcher::AllOf(vec![
            Matcher::Regex(r#"name="file"; filename="audio.wav""#.into()),
            Matcher::Regex(r#"name="model"\r\n\r\nSystran/small"#.into()),
            Matcher::Regex(r#"name="response_format"\r\n\r\nverbose_json"#.into()),
            Matcher::Regex(r#"name="language"\r\n\r\nes"#.into()),
        ]))
        .with_status(200)
        .with_body(GOOD)
        .create_async()
        .await;
    let mut c = config(&server.url());
    c.api_key = "sekrit".into();
    c.model = "Systran/small".into();
    let t = Transcriber::new(&c)
        .transcribe(b"RIFFfake".to_vec(), Some("es"))
        .await
        .unwrap();
    m.assert_async().await;
    assert_eq!(t.language.as_deref(), Some("es"));
    assert_eq!(t.cues.len(), 1);
    assert_eq!((t.cues[0].start_ms, t.cues[0].end_ms), (500, 2000));
}

#[tokio::test]
async fn no_key_no_model_no_language_means_no_such_header_or_field() {
    let mut server = mockito::Server::new_async().await;
    let m = server
        .mock("POST", "/v1/audio/transcriptions")
        .match_header("authorization", Matcher::Missing)
        .match_request(|req| {
            let body = String::from_utf8_lossy(req.body().unwrap()).into_owned();
            body.contains(r#"name="response_format""#)
                && !body.contains(r#"name="model""#)
                && !body.contains(r#"name="language""#)
        })
        .with_status(200)
        .with_body(r#"{"language":"en","segments":[]}"#)
        .create_async()
        .await;
    let t = Transcriber::new(&config(&server.url()))
        .transcribe(b"RIFF".to_vec(), None)
        .await
        .unwrap();
    m.assert_async().await;
    assert!(t.cues.is_empty(), "no speech");
}

#[tokio::test]
async fn a_full_url_is_used_as_typed() {
    let mut server = mockito::Server::new_async().await;
    let m = server
        .mock("POST", "/inference")
        .with_status(200)
        .with_body(GOOD)
        .create_async()
        .await;
    Transcriber::new(&config(&format!("{}/inference", server.url())))
        .transcribe(b"RIFF".to_vec(), None)
        .await
        .unwrap();
    m.assert_async().await;
}

#[tokio::test]
async fn an_srt_answer_is_used_with_the_configured_language() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", Matcher::Any)
        .with_status(200)
        .with_body("1\n00:00:01,000 --> 00:00:02,500\nBonjour\n")
        .expect(2)
        .create_async()
        .await;
    let c = config(&server.url());
    let err = Transcriber::new(&c)
        .transcribe(b"RIFF".to_vec(), None)
        .await
        .unwrap_err();
    assert_eq!(err, TranscribeError::NoLanguage);
    assert!(err.to_string().contains("set transcription.language"));

    let mut c = c;
    c.language = "French".into();
    let t = Transcriber::new(&c)
        .transcribe(b"RIFF".to_vec(), None)
        .await
        .unwrap();
    assert_eq!(t.language.as_deref(), Some("fr"));
    assert_eq!(t.cues[0].text, "Bonjour");
}

#[tokio::test]
async fn an_error_status_gives_the_server_message_with_the_key_scrubbed() {
    let mut server = mockito::Server::new_async().await;
    let long = "x".repeat(500);
    server
        .mock("POST", Matcher::Any)
        .with_status(401)
        .with_body(format!(
            r#"{{"error":{{"message":"bad key sekrit-key-123 {long}"}}}}"#
        ))
        .create_async()
        .await;
    let mut c = config(&server.url());
    c.api_key = "sekrit-key-123".into();
    let err = Transcriber::new(&c)
        .transcribe(b"RIFF".to_vec(), None)
        .await
        .unwrap_err();
    let TranscribeError::Http { status, message } = &err else {
        panic!("{err:?}")
    };
    assert_eq!(*status, 401);
    assert!(!err.to_string().contains("sekrit-key-123"), "{err}");
    assert!(message.starts_with("bad key ********"), "{message}");
    assert!(message.chars().count() <= 300, "truncated");
}

#[tokio::test]
async fn a_slow_server_times_out() {
    // A listener that accepts and never answers
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let mut c = config(&url);
    c.timeout_secs = 10;
    let mut t = Transcriber::new(&c);
    // The config range starts at 10 s; shorten it for this test through the probe's client
    t = t.with_timeout(std::time::Duration::from_millis(300), 1);
    let err = t.transcribe(b"RIFF".to_vec(), None).await.unwrap_err();
    assert_eq!(err, TranscribeError::Timeout(1));
    assert_eq!(err.to_string(), "timeout after 1s");
    drop(listener);
}

#[tokio::test]
async fn an_unreachable_server_says_so_without_the_url_credentials() {
    let mut c = config("http://user:pw@127.0.0.1:1");
    c.api_key = "k".into();
    let err = Transcriber::new(&c)
        .transcribe(b"RIFF".to_vec(), None)
        .await
        .unwrap_err();
    assert!(matches!(err, TranscribeError::Unreachable(_)), "{err:?}");
    assert!(!err.to_string().contains("pw"), "{err}");
}

#[tokio::test]
async fn the_probe_reports_status_time_and_language() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", Matcher::Any)
        .with_status(200)
        .with_body(r#"{"language":"en","text":"","segments":[]}"#)
        .create_async()
        .await;
    let r = Transcriber::new(&config(&server.url()))
        .probe(b"RIFF".to_vec())
        .await;
    assert!(r.ok);
    assert_eq!(r.status, Some(200));
    assert_eq!(r.language.as_deref(), Some("en"));
    assert_eq!(r.error, None);

    let mut bad = mockito::Server::new_async().await;
    bad.mock("POST", Matcher::Any)
        .with_status(500)
        .with_body("model not loaded")
        .create_async()
        .await;
    let r = Transcriber::new(&config(&bad.url()))
        .probe(b"RIFF".to_vec())
        .await;
    assert!(!r.ok);
    assert_eq!(r.status, Some(500));
    assert_eq!(r.error.as_deref(), Some("HTTP 500: model not loaded"));
}
