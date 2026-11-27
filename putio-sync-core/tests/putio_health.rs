//! `putio.unreachable`, `putio.recovered` and `auth.expired` from a client's calls against
//! a mock Put.io (feature 057). One test: the event bus is process-wide.

use std::sync::Arc;
use std::time::Duration;

use putio_sync_client::{PutioApiClient, RetryPolicy};
use putio_sync_core::events::{self, Event, EventType, Payload};
use putio_sync_core::health::PutioWatch;
use tokio::sync::broadcast::Receiver;

const LIST_BODY: &str = r#"{"files":[],"parent":{"id":0,"name":"Root","size":0,"file_type":"FOLDER","parent_id":0,"is_folder":true,"is_shared":false},"total":0,"status":"OK"}"#;

fn seen(rx: &mut Receiver<Arc<Event>>) -> Vec<Arc<Event>> {
    let mut out = Vec::new();
    while let Ok(event) = rx.try_recv() {
        out.push(event);
    }
    out
}

fn types(events: &[Arc<Event>]) -> Vec<EventType> {
    events.iter().map(|e| e.payload.event_type()).collect()
}

async fn answer(server: &mut mockito::ServerGuard, status: usize) -> mockito::Mock {
    server
        .mock("GET", "/v2/files/list")
        .match_query(mockito::Matcher::Any)
        .with_status(status)
        .with_header("content-type", "application/json")
        .with_body(if status == 200 { LIST_BODY } else { "{}" })
        .create_async()
        .await
}

#[tokio::test]
async fn an_outage_and_a_refused_token_are_each_sent_once() {
    let mut server = mockito::Server::new_async().await;
    let client = PutioApiClient::with_base_url("tok_SECRET".into(), format!("{}/v2", server.url()))
        .with_retry_policy(RetryPolicy {
            max_attempts: 2,
            initial_delay: Duration::from_millis(1),
            max_delay: Duration::from_millis(2),
        });
    // Announce on the first failed call, so the test does not wait a minute
    PutioWatch::new(Duration::ZERO).attach(&client);
    let mut rx = events::subscribe();

    // Put.io fails: two calls of two attempts each, one event
    let down = answer(&mut server, 503).await;
    assert!(client.list_files(0).await.is_err());
    assert!(client.list_files(0).await.is_err());
    let events = seen(&mut rx);
    assert_eq!(types(&events), vec![EventType::PutioUnreachable]);
    let Payload::PutioUnreachable(d) = &events[0].payload else {
        unreachable!()
    };
    assert_eq!(d.error, "server_error");
    let since = d.since;
    down.remove_async().await;

    // It answers again: one recovered, with the same start
    let up = answer(&mut server, 200).await;
    client.list_files(0).await.unwrap();
    client.list_files(0).await.unwrap();
    let events = seen(&mut rx);
    assert_eq!(types(&events), vec![EventType::PutioRecovered]);
    let Payload::PutioRecovered(d) = &events[0].payload else {
        unreachable!()
    };
    assert_eq!(d.since, since);
    assert!(d.outage_seconds >= 0.0);
    up.remove_async().await;

    // The token is refused: once, however many calls follow
    let refused = answer(&mut server, 401).await;
    assert!(client.list_files(0).await.is_err());
    assert!(client.list_files(0).await.is_err());
    let events = seen(&mut rx);
    assert_eq!(types(&events), vec![EventType::AuthExpired]);
    let text = serde_json::to_string(&*events[0]).unwrap();
    assert!(!text.contains("tok_SECRET"));
    refused.remove_async().await;

    // A working token, then a refused one again: sent again
    let up = answer(&mut server, 200).await;
    client.list_files(0).await.unwrap();
    up.remove_async().await;
    let _refused = answer(&mut server, 401).await;
    assert!(client.list_files(0).await.is_err());
    assert_eq!(types(&seen(&mut rx)), vec![EventType::AuthExpired]);
}
