//! The Logs page API (feature 047): recent lines, the live tail, level changes, and no
//! secret in any of them.
//!
//! The process logs through `init_logging`, as the daemon does, so lines reach the
//! buffer and the level can change. The buffer and level are process-wide, so the tests
//! take turns.

mod common;

use std::collections::BTreeMap;
use std::time::Duration;

use axum::http::{Method, StatusCode};
use common::{body_json, body_text, test_app, test_app_with, API_TOKEN, PUTIO_TOKEN};
use futures_util::StreamExt;
use putio_sync_core::logging::{init_logging, LoggingConfig};
use serde_json::{json, Value};

/// Every test takes its turn: the log capture, the level and the redaction
/// secrets are process-wide, and building an app replaces the secrets.
static TURN: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Log to a file in a temp dir (quiet), with the capture layer and a reload handle
fn logging() {
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        let dir = Box::leak(Box::new(tempfile::tempdir().unwrap()));
        let config = LoggingConfig {
            level: "info".to_string(),
            console_enabled: false,
            file_enabled: true,
            file_path: dir.path().join("test.log"),
            ..Default::default()
        };
        init_logging(&config).unwrap();
    });
}

async fn lines(app: &common::TestApp, query: &str) -> Vec<Value> {
    let res = app.get(&format!("/logs?{query}")).await;
    assert_eq!(res.status(), StatusCode::OK);
    body_json(res).await["lines"].as_array().unwrap().clone()
}

fn messages(lines: &[Value]) -> Vec<String> {
    lines
        .iter()
        .map(|l| l["message"].as_str().unwrap().to_string())
        .collect()
}

/// Read the stream until `until` appears in it or 5 s pass; returns the text read.
async fn read_stream(app: &common::TestApp, query: &str, until: &str) -> String {
    let res = app.get(&format!("/logs/stream?{query}")).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        res.headers()["content-type"].to_str().unwrap(),
        "text/event-stream"
    );
    let mut body = res.into_body().into_data_stream();
    let mut text = String::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(Ok(chunk)) = body.next().await {
            text.push_str(&String::from_utf8_lossy(&chunk));
            if text.contains(until) {
                break;
            }
        }
    })
    .await;
    text
}

#[tokio::test]
async fn logs_need_a_token() {
    let _turn = TURN.lock().await;
    let app = test_app().await;
    for path in ["/logs", "/logs/stream", "/logs/level"] {
        let res = app.req(Method::GET, path, None, None).await;
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "{path}");
    }
    let res = app
        .req(
            Method::PUT,
            "/logs/level",
            None,
            Some(json!({"level": "trace"})),
        )
        .await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn recent_lines_are_filtered() {
    let _turn = TURN.lock().await;
    logging();
    let app = test_app().await;
    tracing::error!(target: "logs_test::disk", file_id = 42, "filters-case disk full");
    tracing::warn!(target: "logs_test::slow", "filters-case slow");
    tracing::info!(target: "logs_test_other", "filters-case other");
    tracing::debug!(target: "logs_test::disk", "filters-case not kept at info");

    let all = lines(&app, "q=filters-case").await;
    assert_eq!(
        messages(&all),
        [
            "filters-case disk full",
            "filters-case slow",
            "filters-case other"
        ]
    );
    assert_eq!(all[0]["level"], json!("ERROR"));
    assert_eq!(all[0]["fields"], json!([["file_id", "42"]]));

    let warn = lines(&app, "q=filters-case&level=warn").await;
    assert_eq!(messages(&warn).len(), 2);
    let target = lines(&app, "q=filters-case&target=logs_test").await;
    assert_eq!(messages(&target).len(), 2);
    let last = lines(&app, "q=FILTERS-CASE&limit=1").await;
    assert_eq!(messages(&last), ["filters-case other"]);

    let body = body_json(app.get("/logs?limit=1").await).await;
    assert!(body["last_seq"].as_u64().unwrap() >= 3);
    assert!(body["capacity"].as_u64().unwrap() >= 100);
    assert!(body["targets"]
        .as_array()
        .unwrap()
        .contains(&json!("logs_test::disk")));

    let res = app.get("/logs?level=loud").await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn the_stream_sends_the_backlog_then_new_lines() {
    let _turn = TURN.lock().await;
    logging();
    let app = test_app().await;
    tracing::info!(target: "logs_test", "stream-case first");
    let seq = lines(&app, "q=stream-case%20first").await[0]["seq"]
        .as_u64()
        .unwrap();

    let reader = {
        let app = &app;
        async move {
            read_stream(
                app,
                &format!("after={}&q=stream-case", seq - 1),
                "stream-case second",
            )
            .await
        }
    };
    let writer = async {
        tokio::time::sleep(Duration::from_millis(200)).await;
        tracing::info!(target: "logs_test", "stream-case second");
    };
    let (text, ()) = tokio::join!(reader, writer);

    assert!(text.contains("event: line"), "{text}");
    let first = text.find("stream-case first").expect(&text);
    let second = text.find("stream-case second").expect(&text);
    assert!(first < second);
    assert_eq!(text.matches("stream-case first").count(), 1);
}

#[tokio::test]
async fn a_stream_from_an_evicted_line_reports_the_skip() {
    let _turn = TURN.lock().await;
    logging();
    let app = test_app().await;
    putio_sync_core::log_capture::set_capacity(100);
    for i in 0..150 {
        tracing::info!(target: "logs_test", "evict-case {i}");
    }
    let oldest = body_json(app.get("/logs?limit=1").await).await["oldest_seq"]
        .as_u64()
        .unwrap();
    assert!(oldest > 1);
    let text = read_stream(&app, "after=0&q=evict-case", "evict-case 149").await;
    putio_sync_core::log_capture::set_capacity(2000);

    assert!(text.starts_with("event: skipped"), "{text}");
    assert!(
        text.contains(&format!("\"count\":{}", oldest - 1)),
        "{text}"
    );
}

#[tokio::test]
async fn the_level_can_be_changed_and_reverted() {
    let _turn = TURN.lock().await;
    logging();
    let app = test_app().await;
    let put = |body: Value| {
        let app = &app;
        async move {
            app.req(Method::PUT, "/logs/level", Some(API_TOKEN), Some(body))
                .await
        }
    };

    for body in [
        json!({"level": "loud"}),
        json!({"target": "not a module", "level": "debug"}),
        json!({"target": "logs_test", "level": "debug", "minutes": 0}),
        json!({"target": "logs_test", "level": "debug", "minutes": 2000}),
    ] {
        let res = put(body.clone()).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{body}");
    }

    tracing::debug!(target: "logs_test::level", "level-case before");
    let res = put(json!({"target": "logs_test", "level": "debug", "minutes": 30})).await;
    assert_eq!(res.status(), StatusCode::OK);
    let status = body_json(res).await;
    assert_eq!(status["modules"][0]["target"], json!("logs_test"));
    assert!(status["modules"][0]["until"].is_string());
    assert_eq!(status["verbose"], json!(true));
    assert_eq!(status["available"], json!(true));

    tracing::debug!(target: "logs_test::level", "level-case during");
    tracing::debug!(target: "other_module", "level-case other");
    assert_eq!(
        messages(&lines(&app, "q=level-case").await),
        ["level-case during"]
    );

    let shown = body_json(app.get("/logs/level").await).await;
    assert!(shown["effective"]
        .as_str()
        .unwrap()
        .contains("logs_test=debug"));

    let res = app
        .req(
            Method::DELETE,
            "/logs/level?target=logs_test",
            Some(API_TOKEN),
            None,
        )
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    let status = body_json(res).await;
    assert_eq!(status["modules"], json!([]));
    assert_eq!(status["verbose"], json!(false));

    tracing::debug!(target: "logs_test::level", "level-case after");
    assert_eq!(
        messages(&lines(&app, "q=level-case").await),
        ["level-case during"]
    );
    // The change itself is logged
    assert!(!lines(
        &app,
        "q=Log%20level%20changed&target=putio_sync_core::logging"
    )
    .await
    .is_empty());
}

#[tokio::test]
async fn secrets_never_reach_the_viewer() {
    let _turn = TURN.lock().await;
    logging();
    let webhook = "https://ha.lan/api/webhook/s3cret-webhook-id";
    let hook_url = "https://hooks.lan/run/s3cret-hook-path";
    let app = test_app_with(|config| {
        config.arr.password = Some("s3cret-arr-password".to_string());
        config.notifications.webhook_url = Some(webhook.to_string());
        let mut hook = putio_sync_core::hooks::HookConfig {
            name: "ha".to_string(),
            events: vec!["*".to_string()],
            url: Some(hook_url.to_string()),
            ..Default::default()
        };
        hook.headers = BTreeMap::from([("X-Secret".to_string(), "s3cret-hook-header".to_string())]);
        config.hooks = vec![hook];
        let mut plugin = putio_sync_core::plugins::PluginSettings {
            name: "media".to_string(),
            ..Default::default()
        };
        plugin.secrets = BTreeMap::from([("key".to_string(), "s3cret-plugin-key".to_string())]);
        config.plugins = vec![plugin];
    })
    .await;
    // What is logged; the secret parts of the URLs are checked as well
    let logged = [
        PUTIO_TOKEN,
        API_TOKEN,
        "s3cret-arr-password",
        webhook,
        hook_url,
        "s3cret-hook-header",
        "s3cret-plugin-key",
    ];
    let secrets = logged.iter().copied().chain([
        "s3cret-webhook-id",
        "s3cret-hook-path",
        "bearer-credential-77",
    ]);
    let first = body_json(app.get("/logs?limit=1").await).await["last_seq"]
        .as_u64()
        .unwrap();

    let reader = {
        let app = &app;
        async move {
            read_stream(
                app,
                &format!("after={first}&q=secrets-case"),
                "secrets-case end",
            )
            .await
        }
    };
    let writer = async {
        tokio::time::sleep(Duration::from_millis(200)).await;
        for secret in logged {
            tracing::info!(target: "logs_test", case = "secrets-case", "sent {secret} along");
            tracing::info!(target: "logs_test", case = "secrets-case", value = %secret, "secrets-case field");
        }
        tracing::info!(target: "logs_test", case = "secrets-case", "header Authorization: Bearer bearer-credential-77");
        tracing::info!(target: "logs_test", "secrets-case end");
    };
    let (streamed, ()) = tokio::join!(reader, writer);

    let res = app.get("/logs?q=secrets-case").await;
    let kept = body_text(res).await;
    assert!(kept.contains("secrets-case field"), "{kept}");
    assert!(streamed.contains("secrets-case end"), "{streamed}");
    for secret in secrets {
        assert!(!kept.contains(secret), "{secret} in GET /logs");
        assert!(!streamed.contains(secret), "{secret} in the stream");
    }
    assert!(kept.contains("Authorization: ********"), "{kept}");
}
