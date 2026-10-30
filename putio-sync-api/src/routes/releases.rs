//! Release pre-check routes (feature 042, contracts/api.md)
//!
//! - GET /releases/reports?range=&format=json|csv - Every rejection and flag in the range

use crate::error::ApiError;
use crate::AppState;
use axum::{
    extract::{Query, State},
    http::header,
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use putio_sync_db::analytics::RangePreset;
use putio_sync_db::ReportRow;
use serde::Deserialize;
use tracing::error;

/// Create the releases router
pub fn router() -> Router<AppState> {
    Router::new().route("/reports", get(reports))
}

/// `GET /releases/reports` query parameters; empty values count as absent
#[derive(Debug, Default, Deserialize)]
pub struct ReportParams {
    pub range: Option<String>,
    pub format: Option<String>,
}

/// The CSV columns, in order
const COLUMNS: [&str; 22] = [
    "id",
    "kind",
    "check_id",
    "arr_hash",
    "file_id",
    "transfer_id",
    "source",
    "category",
    "watch_folder",
    "stage",
    "name",
    "hash",
    "total_size",
    "file_count",
    "types",
    "largest_wanted",
    "min_size",
    "tracker_hosts",
    "is_private",
    "reason",
    "note",
    "created_at",
];

/// Download the reports made in a range (everything by default)
///
/// GET /releases/reports?range=24h|7d|30d|90d|1y|all&format=json|csv
async fn reports(
    State(state): State<AppState>,
    Query(params): Query<ReportParams>,
) -> Result<Response, ApiError> {
    let invalid = |e: &str| ApiError::BadRequest("Invalid query".to_string()).with_details(e);
    let range = match params.range.as_deref().filter(|r| !r.is_empty()) {
        None => RangePreset::All,
        Some(r) => r.parse().map_err(|e: String| invalid(&e))?,
    };
    let csv = match params.format.as_deref().filter(|f| !f.is_empty()) {
        None | Some("json") => false,
        Some("csv") => true,
        Some(_) => return Err(invalid("format must be json or csv")),
    };
    let db = state
        .database
        .as_ref()
        .ok_or_else(|| ApiError::InternalError("Database not available".to_string()))?;

    let rows = db
        .releases()
        .reports(range, chrono::Utc::now().timestamp())
        .await
        .map_err(|e| {
            error!("Failed to list release reports: {}", e);
            ApiError::InternalError("Failed to list release reports".to_string())
        })?;
    let ext = if csv { "csv" } else { "json" };
    let disposition = format!(
        "attachment; filename=\"putio-sync-releases-{}.{ext}\"",
        range.as_str()
    );
    if csv {
        let headers = [
            (header::CONTENT_TYPE, "text/csv; charset=utf-8".to_string()),
            (header::CONTENT_DISPOSITION, disposition),
        ];
        Ok((headers, to_csv(&rows)).into_response())
    } else {
        Ok(([(header::CONTENT_DISPOSITION, disposition)], Json(rows)).into_response())
    }
}

/// One row per report; `types` as `ext:count:largest;…` and `tracker_hosts` joined by `;`
fn to_csv(rows: &[ReportRow]) -> String {
    let mut out = COLUMNS.join(",");
    out.push('\n');
    let opt = |v: Option<i64>| v.map(|n| n.to_string()).unwrap_or_default();
    let text = |v: &Option<String>| v.clone().unwrap_or_default();
    for r in rows {
        let types: Vec<String> = r
            .types
            .iter()
            .map(|t| format!("{}:{}:{}", t.ext, t.count, t.largest))
            .collect();
        let fields = [
            r.id.to_string(),
            r.kind.clone(),
            opt(r.check_id),
            text(&r.arr_hash),
            opt(r.file_id),
            opt(r.transfer_id),
            text(&r.source),
            text(&r.category),
            text(&r.watch_folder),
            text(&r.stage),
            r.name.clone(),
            text(&r.hash),
            opt(r.total_size),
            opt(r.file_count),
            types.join(";"),
            opt(r.largest_wanted),
            opt(r.min_size),
            r.tracker_hosts.join(";"),
            r.is_private.to_string(),
            text(&r.reason),
            text(&r.note),
            r.created_at.clone(),
        ];
        let line: Vec<String> = fields.iter().map(|f| field(f)).collect();
        out.push_str(&line.join(","));
        out.push('\n');
    }
    out
}

/// A CSV field, quoted when it holds a comma, quote or line break. A leading `=`, `+`,
/// `-` or `@` gets a `'` so a spreadsheet doesn't run a release name as a formula.
fn field(value: &str) -> String {
    let value = match value.chars().next() {
        Some('=' | '+' | '-' | '@') => format!("'{value}"),
        _ => value.to_string(),
    };
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_are_quoted_and_defused() {
        assert_eq!(field("Movie.2024"), "Movie.2024");
        assert_eq!(field("a,b"), "\"a,b\"");
        assert_eq!(field("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(field("=cmd()"), "'=cmd()");
        assert_eq!(field("-1"), "'-1");
    }
}
