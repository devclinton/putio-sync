//! `POST /translate/callback/{token}` (feature 026, contract callback.md)
//!
//! Public: the one-time token in the path is the credential. Every refusal of the token looks
//! the same (404), failures count against the caller's address like a bad sign-in, and the
//! token is masked wherever the path is logged.

use crate::error::ApiError;
use crate::middleware::ratelimit::client_ip;
use crate::AppState;
use axum::{
    extract::{Path, Request, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use putio_sync_pipeline::callback::AcceptError;
use tracing::warn;

/// The largest result accepted
pub const MAX_BODY: usize = 4 * 1024 * 1024;

fn unknown() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({"error": "unknown or expired callback"})),
    )
        .into_response()
}

pub async fn callback(
    State(state): State<AppState>,
    Path(token): Path<String>,
    request: Request,
) -> Response {
    let ip = client_ip(&request);
    if let Err(secs) = state.limiter.check(ip) {
        warn!(ip = %ip, retry_after_seconds = secs, "Translation callback rate limited");
        return ApiError::TooManyRequests {
            retry_after_seconds: secs,
        }
        .into_response();
    }
    let Some(pipeline) = state.pipeline.clone() else {
        return unknown();
    };
    let content_type = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let bytes = match axum::body::to_bytes(request.into_body(), MAX_BODY).await {
        Ok(b) => b,
        Err(_) => {
            return (
                StatusCode::PAYLOAD_TOO_LARGE,
                Json(serde_json::json!({"error": "the result is larger than 4 MiB"})),
            )
                .into_response()
        }
    };
    let body = String::from_utf8_lossy(&bytes);
    match pipeline
        .accept_callback(&token, content_type.as_deref(), &body)
        .await
    {
        Ok(()) => Json(serde_json::json!({"ok": true})).into_response(),
        Err(AcceptError::NotFound) => {
            let failures = state.limiter.record_failure(ip);
            warn!(ip = %ip, failures, "Translation callback refused");
            unknown()
        }
        Err(AcceptError::Unprocessable(message)) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({"error": message})),
        )
            .into_response(),
    }
}
