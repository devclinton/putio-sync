//! Logs page routes (feature 047)
//!
//! - GET /logs - Recent lines kept in memory, filtered
//! - GET /logs/stream - New lines as server-sent events (live tail)
//! - GET /logs/level - The level in effect and the page's overrides
//! - PUT /logs/level - Set the overall or a module's level, for some minutes or until restart
//! - DELETE /logs/level - Revert one module (`?target=`), the overall level (`?target=`
//!   empty), or everything
//!
//! Every line was masked by `putio_sync_core::log_redact` when it was kept.

use std::collections::VecDeque;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::{rejection::JsonRejection, Query},
    response::sse::{Event, KeepAlive, Sse},
    routing::get,
    Json, Router,
};
use futures_util::stream::Stream;
use putio_sync_core::log_capture::{self, LogLine, LogQuery};
use putio_sync_core::logging::{self, LevelError, LevelStatus};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast::{self, error::RecvError};

use crate::error::ApiError;
pub use crate::AppState;

/// Create the Logs router
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(get_logs))
        .route("/stream", get(stream))
        .route("/level", get(get_level).put(put_level).delete(delete_level))
}

/// Filters shared by `GET /logs` and the stream
#[derive(Debug, Default, Deserialize)]
struct Params {
    after: Option<u64>,
    level: Option<String>,
    target: Option<String>,
    q: Option<String>,
    limit: Option<usize>,
}

impl Params {
    fn query(self) -> Result<LogQuery, ApiError> {
        let level = match self.level.as_deref().map(str::trim) {
            None | Some("") => None,
            Some(level) => Some(
                ["error", "warn", "info", "debug", "trace"]
                    .contains(&level.to_ascii_lowercase().as_str())
                    .then(|| level.parse::<tracing::Level>().ok())
                    .flatten()
                    .ok_or_else(|| {
                        ApiError::BadRequest(
                            "level must be one of error, warn, info, debug, trace".to_string(),
                        )
                    })?,
            ),
        };
        Ok(LogQuery {
            after: self.after,
            level,
            target: self.target.map(|t| t.trim().to_string()),
            q: self.q,
            limit: self.limit.map(|l| l.max(1)),
        })
    }
}

#[derive(Serialize)]
struct LogsResponse {
    lines: Vec<Arc<LogLine>>,
    last_seq: u64,
    oldest_seq: u64,
    capacity: usize,
    evicted: u64,
    targets: Vec<String>,
}

async fn get_logs(Query(params): Query<Params>) -> Result<Json<LogsResponse>, ApiError> {
    let query = params.query()?;
    let lines = log_capture::query(&query);
    let stats = log_capture::stats();
    Ok(Json(LogsResponse {
        lines,
        last_seq: stats.last_seq,
        oldest_seq: stats.oldest_seq,
        capacity: stats.capacity,
        evicted: stats.evicted,
        targets: log_capture::targets(),
    }))
}

/// A live tail: what it has sent up to, and events waiting to go
struct Tail {
    rx: broadcast::Receiver<Arc<LogLine>>,
    filter: LogQuery,
    last: u64,
    pending: VecDeque<Event>,
}

impl Tail {
    /// Queue the kept lines after `last`, and a `skipped` event for those already gone.
    fn catch_up(&mut self) {
        let oldest = log_capture::stats().oldest_seq;
        if oldest > self.last + 1 {
            let count = oldest - self.last - 1;
            putio_sync_telemetry::logs::record_dropped("lagged", count);
            self.pending.push_back(
                Event::default()
                    .event("skipped")
                    .data(serde_json::json!({ "count": count }).to_string()),
            );
        }
        let lines = log_capture::query(&LogQuery {
            after: Some(self.last),
            ..Default::default()
        });
        for line in lines {
            self.offer(&line);
        }
    }

    /// Queue `line` if it is new and matches the filters.
    fn offer(&mut self, line: &LogLine) {
        if line.seq <= self.last {
            return;
        }
        self.last = line.seq;
        if line.matches(&self.filter) {
            let data = serde_json::to_string(line).unwrap_or_default();
            self.pending
                .push_back(Event::default().event("line").data(data));
        }
    }
}

async fn stream(
    Query(params): Query<Params>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let mut filter = params.query()?;
    // Subscribe before reading the backlog, so no line falls between the two
    let rx = log_capture::subscribe();
    let last = filter
        .after
        .take()
        .unwrap_or_else(|| log_capture::stats().last_seq);
    filter.limit = None;
    let mut tail = Tail {
        rx,
        filter,
        last,
        pending: VecDeque::new(),
    };
    tail.catch_up();

    let events = futures_util::stream::unfold(tail, |mut tail| async move {
        loop {
            if let Some(event) = tail.pending.pop_front() {
                return Some((Ok(event), tail));
            }
            match tail.rx.recv().await {
                Ok(line) => tail.offer(&line),
                Err(RecvError::Lagged(_)) => tail.catch_up(),
                Err(RecvError::Closed) => return None,
            }
        }
    });
    Ok(Sse::new(events).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}

fn level_error(e: LevelError) -> ApiError {
    match e {
        LevelError::Invalid(msg) => ApiError::BadRequest(msg),
        LevelError::Unavailable => ApiError::Conflict(e.to_string()),
    }
}

async fn get_level() -> Json<LevelStatus> {
    Json(logging::level_status())
}

#[derive(Debug, Deserialize)]
struct LevelRequest {
    #[serde(default)]
    target: Option<String>,
    level: String,
    /// Left out or null: keep until restart
    #[serde(default)]
    minutes: Option<u32>,
}

async fn put_level(
    body: Result<Json<LevelRequest>, JsonRejection>,
) -> Result<Json<LevelStatus>, ApiError> {
    let Json(request) = body.map_err(|r| ApiError::BadRequest(r.body_text()))?;
    logging::set_override(request.target.as_deref(), &request.level, request.minutes)
        .map(Json)
        .map_err(level_error)
}

#[derive(Debug, Deserialize)]
struct ClearParams {
    target: Option<String>,
}

async fn delete_level(Query(params): Query<ClearParams>) -> Result<Json<LevelStatus>, ApiError> {
    logging::clear_override(params.target.as_deref())
        .map(Json)
        .map_err(level_error)
}
