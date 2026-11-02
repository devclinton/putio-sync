//! Release pre-check routes (feature 042, contracts/api.md)
//!
//! - POST /releases/flags - Flag a release "Fake" or "Not fake", optionally failing and
//!   blocklisting an *arr item
//! - DELETE /releases/flags/{id} - Undo a flag
//! - GET /releases/flags?arr_hash=&file_id=&history_id=&report_id= - Flags on those rows
//! - GET /releases/reports?range=&format=json|csv - Every rejection and flag in the range

use crate::error::ApiError;
use crate::AppState;
use axum::{
    extract::{Path, Query, RawQuery, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get},
    Json, Router,
};
use putio_sync_core::rules::RuleSet;
use putio_sync_db::analytics::RangePreset;
use putio_sync_db::models::Origin;
use putio_sync_db::{ArrItem, Database, FlagUndo, NewReport, ReportKind, ReportRow};
use putio_sync_precheck::policy::Key;
use putio_sync_precheck::{report, Policy, ReleaseFile, Traits};
use putio_sync_telemetry::precheck as metrics;
use putio_sync_telemetry::FlagKind;
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::{error, info};

/// The most targets one `GET /releases/flags` may ask about
const MAX_TARGETS: usize = 200;

/// Create the releases router
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/flags", get(list_flags).post(create_flag))
        .route("/flags/:id", delete(undo_flag))
        .route("/reports", get(reports))
}

/// What a flag points at: the row the operator clicked
#[derive(Debug, Clone, PartialEq)]
enum Target {
    Arr(String),
    File(i64),
    History(i64),
    /// A rejection, flagged "Not fake" from History's rejected releases
    Report(i64),
}

impl Target {
    /// `{"arr_hash": "…"}`, `{"file_id": 1}`, `{"history_id": 1}` or `{"report_id": 1}`
    fn parse(value: &Value) -> Option<Target> {
        let map = value.as_object().filter(|m| m.len() == 1)?;
        let (key, value) = map.iter().next()?;
        match key.as_str() {
            "arr_hash" => value
                .as_str()
                .filter(|h| !h.is_empty())
                .map(|h| Target::Arr(h.to_ascii_lowercase())),
            "file_id" => value.as_i64().map(Target::File),
            "history_id" => value.as_i64().map(Target::History),
            "report_id" => value.as_i64().map(Target::Report),
            _ => None,
        }
    }

    fn to_json(&self) -> Value {
        match self {
            Target::Arr(h) => json!({ "arr_hash": h }),
            Target::File(id) => json!({ "file_id": id }),
            Target::History(id) => json!({ "history_id": id }),
            Target::Report(id) => json!({ "report_id": id }),
        }
    }
}

/// `POST /releases/flags` body
#[derive(Debug, Deserialize)]
pub struct FlagBody {
    pub target: Value,
    pub kind: String,
    pub note: Option<String>,
    #[serde(default)]
    pub blocklist: bool,
}

/// What a flag records about its release, before the traits
#[derive(Debug, Default)]
struct Release {
    arr: Option<ArrItem>,
    file_id: Option<i64>,
    transfer_id: Option<i64>,
    name: String,
    hash: Option<String>,
    source: Option<String>,
    category: Option<String>,
    watch_folder: Option<String>,
    tracker_host: Option<String>,
    private: bool,
    /// The files from History, for when Put.io no longer has them
    known: Vec<ReleaseFile>,
    /// The rejection a "Not fake" flag answers
    rejection: Option<ReportRow>,
}

fn db(state: &AppState) -> Result<&Database, ApiError> {
    state
        .database
        .as_deref()
        .ok_or_else(|| ApiError::InternalError("Database not available".to_string()))
}

fn db_error(what: &'static str) -> impl Fn(putio_sync_db::DbError) -> ApiError {
    move |e| {
        error!("{what}: {e}");
        ApiError::InternalError(what.to_string())
    }
}

/// Flag a release (contracts/api.md `POST /releases/flags`)
async fn create_flag(
    State(state): State<AppState>,
    Json(body): Json<FlagBody>,
) -> Result<Response, ApiError> {
    let bad = |e: &str| ApiError::BadRequest("Invalid flag".to_string()).with_details(e);
    let target = Target::parse(&body.target)
        .ok_or_else(|| bad("target must be one of arr_hash, file_id, history_id or report_id"))?;
    let fake = match body.kind.as_str() {
        "fake" => true,
        "not_fake" => false,
        _ => return Err(bad("kind must be fake or not_fake")),
    };
    let note = body
        .note
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty());
    if note.is_some_and(|n| n.chars().count() > putio_sync_db::releases::MAX_NOTE_CHARS) {
        return Err(bad("note must be at most 500 characters"));
    }
    if body.blocklist && !fake {
        return Err(bad("only a fake flag can fail and blocklist"));
    }
    let db = db(&state)?;
    let release = resolve(&state, db, &target).await?;

    if !fake {
        let rejected = release.rejection.is_some()
            || db
                .releases()
                .was_rejected(
                    release.arr.as_ref().map(|a| a.hash.as_str()),
                    release.hash.as_deref(),
                )
                .await
                .map_err(db_error("Failed to look up the release"))?;
        if !rejected {
            return Err(bad("only a rejected release can be flagged not fake"));
        }
    } else if release.rejection.is_some() {
        return Err(bad("a rejected release is already counted as fake"));
    }
    let arr = match (&release.arr, &state.arr) {
        (Some(item), Some(rpc)) => Some((item, rpc)),
        _ => None,
    };
    if body.blocklist && arr.is_none() {
        return Err(bad(
            "only a Radarr or Sonarr item can be failed and blocklisted",
        ));
    }

    let traits = match &release.rejection {
        Some(_) => None,
        None => {
            let policy = policy(&state, &release).await;
            report::traits_for(
                &state.client,
                release.file_id,
                release.known.clone(),
                &policy,
            )
            .await
        }
    };
    let id = save(db, &release, traits.as_ref(), fake, note).await?;
    metrics::flag(if fake {
        FlagKind::Fake
    } else {
        FlagKind::NotFake
    });
    info!(
        id,
        kind = body.kind,
        name = %release.name,
        hash = ?release.hash,
        "Release flagged"
    );

    let mut response = json!({
        "id": id,
        "kind": if fake { "flagged_fake" } else { "flagged_not_fake" },
        "name": release.name,
        "blocklisted": false,
    });
    if let (true, Some((item, rpc))) = (body.blocklist, arr) {
        let dry_run = state.config.read().await.precheck.dry_run;
        let queued = rpc
            .service
            .flag_fake(&item.hash, dry_run)
            .await
            .map_err(|e| {
                error!(hash = %item.hash, error = %e, "Failed to fail a flagged arr item");
                ApiError::InternalError("Failed to fail the item".to_string())
            })?;
        response["blocklisted"] = json!(queued);
        if dry_run {
            response["dry_run"] = json!(true);
        }
    }
    Ok((StatusCode::CREATED, Json(response)).into_response())
}

/// Look the target up: 404 when it doesn't exist
async fn resolve(state: &AppState, db: &Database, target: &Target) -> Result<Release, ApiError> {
    let missing = || ApiError::NotFound("No such release".to_string());
    match target {
        Target::Arr(hash) => {
            let item = db
                .arr()
                .get_by_hash(hash)
                .await
                .map_err(db_error("Failed to look up the item"))?
                .ok_or_else(missing)?;
            Ok(from_arr(item, None))
        }
        Target::File(file_id) => file_release(state, db, *file_id, None).await,
        Target::History(id) => {
            let row = db
                .history()
                .get(*id)
                .await
                .map_err(db_error("Failed to look up the history row"))?
                .ok_or_else(missing)?;
            let known = vec![ReleaseFile {
                path: row.filename.clone(),
                size: row.file_size.max(0) as u64,
            }];
            let mut release = file_release(state, db, row.file_id, Some(row.filename)).await?;
            release.known = known;
            Ok(release)
        }
        Target::Report(id) => {
            let report = db
                .releases()
                .report(*id)
                .await
                .map_err(db_error("Failed to look up the report"))?
                .ok_or_else(missing)?;
            if report.kind != ReportKind::Rejected.as_str() {
                return Err(ApiError::BadRequest("Invalid flag".to_string())
                    .with_details("report_id must name a rejection"));
            }
            Ok(Release {
                file_id: report.file_id,
                transfer_id: report.transfer_id,
                name: report.name.clone(),
                hash: report.hash.clone(),
                source: report.source.clone(),
                category: report.category.clone(),
                watch_folder: report.watch_folder.clone(),
                private: report.is_private,
                rejection: Some(report),
                ..Release::default()
            })
        }
    }
}

fn from_arr(item: ArrItem, file_id: Option<i64>) -> Release {
    Release {
        file_id: file_id.or(item.file_id),
        transfer_id: item.transfer_id,
        name: item.name.clone(),
        hash: Some(item.hash.clone()),
        category: Some(item.category.clone()),
        arr: Some(item),
        ..Release::default()
    }
}

/// A Put.io file: an *arr item's when one holds it, else what the sync recorded of it
async fn file_release(
    state: &AppState,
    db: &Database,
    file_id: i64,
    name: Option<String>,
) -> Result<Release, ApiError> {
    if let Some(item) = db
        .arr()
        .get_by_file_id(file_id)
        .await
        .map_err(db_error("Failed to look up the item"))?
    {
        return Ok(from_arr(item, Some(file_id)));
    }
    let source = db
        .sources()
        .get(file_id)
        .await
        .map_err(db_error("Failed to look up the file's source"))?;
    let name = match name {
        Some(name) => name,
        None => match state.client.get_file_details(file_id).await {
            Ok(details) => details.file.name,
            Err(_) => db
                .history()
                .latest_for_file(file_id)
                .await
                .map_err(db_error("Failed to look up the history row"))?
                .map(|row| row.filename)
                .ok_or_else(|| ApiError::NotFound("No such release".to_string()))?,
        },
    };
    let mut release = Release {
        file_id: Some(file_id),
        name,
        ..Release::default()
    };
    if let Some(source) = source {
        release.transfer_id = source.transfer_id;
        release.tracker_host = source.tracker_host;
        release.private = source.is_private;
        release.watch_folder = source.watch_folder;
        release.source = Some(
            match source.origin {
                Origin::Watch => "watch",
                Origin::Manual => "manual",
                Origin::Putio | Origin::Unknown => "putio",
            }
            .to_string(),
        );
    }
    Ok(release)
}

/// The wanted types for the largest-wanted trait: the category's or watch folder's policy
async fn policy(state: &AppState, release: &Release) -> Policy {
    let config = state.config.read().await;
    let rules = RuleSet::from_config(&config.download);
    let watch = release.watch_folder.as_deref().map(std::path::Path::new);
    let key = match (&release.category, watch) {
        (Some(category), _) => Key::Category(category),
        (None, Some(path)) => Key::Watch(path),
        (None, None) => Key::Category(""),
    };
    Policy::for_folder(&config.precheck, &rules, "/", key)
}

async fn save(
    db: &Database,
    release: &Release,
    traits: Option<&Traits>,
    fake: bool,
    note: Option<&str>,
) -> Result<i64, ApiError> {
    let repo = db.releases();
    let check_id = match &release.rejection {
        Some(r) => r.check_id,
        None => repo
            .latest_check(release.hash.as_deref(), release.transfer_id)
            .await
            .map_err(db_error("Failed to look up the release check"))?,
    };
    let (types, tracker_hosts) = match (&release.rejection, traits) {
        (Some(r), _) => (
            serde_json::to_string(&r.types).unwrap_or_else(|_| "[]".to_string()),
            serde_json::to_string(&r.tracker_hosts).unwrap_or_else(|_| "[]".to_string()),
        ),
        (None, Some(t)) => (t.types_json(), {
            let hosts: Vec<&str> = release.tracker_host.iter().map(String::as_str).collect();
            serde_json::to_string(&hosts).unwrap_or_else(|_| "[]".to_string())
        }),
        (None, None) => ("[]".to_string(), {
            let hosts: Vec<&str> = release.tracker_host.iter().map(String::as_str).collect();
            serde_json::to_string(&hosts).unwrap_or_else(|_| "[]".to_string())
        }),
    };
    let r = release.rejection.as_ref();
    let flag = NewReport {
        kind: if fake {
            ReportKind::FlaggedFake
        } else {
            ReportKind::FlaggedNotFake
        },
        check_id,
        arr_hash: release
            .arr
            .as_ref()
            .map(|a| a.hash.as_str())
            .or_else(|| r.and_then(|r| r.arr_hash.as_deref())),
        file_id: release.file_id,
        transfer_id: release.transfer_id,
        source: release.source.as_deref(),
        category: release.category.as_deref(),
        watch_folder: release.watch_folder.as_deref(),
        stage: None,
        name: &release.name,
        hash: release.hash.as_deref(),
        total_size: r
            .and_then(|r| r.total_size)
            .or(traits.map(|t| t.total_size as i64)),
        file_count: r
            .and_then(|r| r.file_count)
            .or(traits.map(|t| t.file_count as i64)),
        types: &types,
        largest_wanted: r
            .and_then(|r| r.largest_wanted)
            .or(traits.map(|t| t.largest_wanted as i64)),
        min_size: r.and_then(|r| r.min_size),
        tracker_hosts: &tracker_hosts,
        is_private: release.private,
        reason: r.and_then(|r| r.reason.as_deref()),
        note,
    };
    repo.upsert_flag(&flag)
        .await
        .map_err(db_error("Failed to save the flag"))
}

/// Undo a flag (contracts/api.md `DELETE /releases/flags/{id}`). A blocklist already sent
/// stays.
async fn undo_flag(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    match db(&state)?
        .releases()
        .delete_flag(id)
        .await
        .map_err(db_error("Failed to undo the flag"))?
    {
        FlagUndo::Deleted => {
            info!(id, "Release flag undone");
            Ok(StatusCode::NO_CONTENT)
        }
        FlagUndo::NotFound => Err(ApiError::NotFound("No such flag".to_string())),
        FlagUndo::NotAFlag => Err(ApiError::Conflict(
            "A rejection by the check can't be undone".to_string(),
        )),
    }
}

/// The flags on the rows on screen (contracts/api.md `GET /releases/flags`). Each
/// parameter may repeat, up to 200 in all.
async fn list_flags(
    State(state): State<AppState>,
    RawQuery(query): RawQuery,
) -> Result<Json<Value>, ApiError> {
    let bad = |e: &str| ApiError::BadRequest("Invalid query".to_string()).with_details(e);
    let mut targets = Vec::new();
    for (key, value) in url::form_urlencoded::parse(query.unwrap_or_default().as_bytes()) {
        if value.is_empty() {
            continue;
        }
        let number = || {
            value
                .parse::<i64>()
                .map_err(|_| bad(&format!("{key} must be a number")))
        };
        targets.push(match key.as_ref() {
            "arr_hash" => Target::Arr(value.to_ascii_lowercase()),
            "file_id" => Target::File(number()?),
            "history_id" => Target::History(number()?),
            "report_id" => Target::Report(number()?),
            _ => return Err(bad(&format!("unknown parameter {key}"))),
        });
    }
    if targets.len() > MAX_TARGETS {
        return Err(bad("at most 200 targets"));
    }
    let db = db(&state)?;

    // Each target matches flags by the key a flag on it is saved under
    let mut keys: Vec<(Target, Match)> = Vec::new();
    for target in targets {
        let key = match &target {
            Target::Arr(h) => Some(Match::Arr(h.clone())),
            Target::File(id) => Some(Match::File(*id)),
            Target::History(id) => db
                .history()
                .get(*id)
                .await
                .map_err(db_error("Failed to look up the history row"))?
                .map(|row| Match::File(row.file_id)),
            Target::Report(id) => db
                .releases()
                .report(*id)
                .await
                .map_err(db_error("Failed to look up the report"))?
                .and_then(|r| match (r.arr_hash, r.file_id, r.hash) {
                    (Some(h), _, _) => Some(Match::Arr(h)),
                    (None, Some(id), _) => Some(Match::File(id)),
                    (None, None, Some(h)) => Some(Match::Hash(h)),
                    _ => r.check_id.map(Match::Check),
                }),
        };
        if let Some(key) = key {
            keys.push((target, key));
        }
    }
    let (mut arr, mut files, mut hashes, mut checks) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for (_, key) in &keys {
        match key {
            Match::Arr(h) => arr.push(h.clone()),
            Match::File(id) => files.push(*id),
            Match::Hash(h) => hashes.push(h.clone()),
            Match::Check(id) => checks.push(*id),
        }
    }
    let rows = db
        .releases()
        .flags_for(&arr, &files, &hashes, &checks)
        .await
        .map_err(db_error("Failed to list flags"))?;
    // Asked-for files an *arr item holds: their flag can also fail and blocklist
    let asked: Vec<i64> = keys
        .iter()
        .filter_map(|(target, _)| match target {
            Target::File(id) => Some(*id),
            _ => None,
        })
        .collect();
    let mut arr_files = db
        .arr()
        .arr_file_ids(&asked)
        .await
        .map_err(db_error("Failed to list flags"))?;
    arr_files.sort_unstable();

    let mut flags = Vec::new();
    for (target, key) in &keys {
        for row in rows.iter().filter(|row| key.matches(row)) {
            flags.push(json!({
                "id": row.id,
                "kind": row.kind,
                "target": target.to_json(),
                "note": row.note,
            }));
        }
    }
    Ok(Json(json!({ "flags": flags, "arr_file_ids": arr_files })))
}

/// The column a target's flags are found by
#[derive(Debug, Clone)]
enum Match {
    Arr(String),
    File(i64),
    Hash(String),
    Check(i64),
}

impl Match {
    fn matches(&self, row: &ReportRow) -> bool {
        match self {
            Match::Arr(h) => row.arr_hash.as_deref() == Some(h),
            Match::File(id) => row.file_id == Some(*id),
            Match::Hash(h) => row.hash.as_deref() == Some(h),
            Match::Check(id) => row.check_id == Some(*id),
        }
    }
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
