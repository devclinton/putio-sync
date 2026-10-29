//! Serve the web UI over a mocked Put.io and a seeded temp database, so every page can
//! be clicked through offline (feature 004, R10).
//!
//! ```text
//! cargo run -p putio-sync-api --example mock_ui [port]
//! ```
//!
//! Prints the URL and the API token to sign in with, then runs until Ctrl+C. The port
//! defaults to a free one. Nothing reaches the real Put.io through `state.client`.
//! The history holds 64 rows (two pages); 7 of them match `ubuntu`. About 120 days of
//! older downloads, evening-heavy, with sources and origins, fill the Stats page.
//! "Remove empty folders" on the Put.io page finds `Movies/Extras` (empty); the mocked
//! delete succeeds but the listing still shows it.
//! On `#/transfers`, transfer 5 is stalled at 97% (feature 007), transfer 1 downloads
//! normally and the failed transfer 4 has no magnet link.
//! Feature 008: tick rows on `#/putio/100` (Big.Buck.Bunny is queued, so its box is
//! disabled) or `#/transfers` for the bulk actions. On `#/transfers`, Clear completed
//! finds 2 (one completed, one seeding), Clear failed 1, Clear all finished 3. Every
//! mocked delete, cancel and clean succeeds, but the listings don't change.
//! Feature 009: `Shows` has a "Video only" rule, so `Example.Show.S01E01.nfo` there shows
//! as blocked; `Movies` and `Movies/Extras` start with the global type filter. Recently
//! blocked on Settings holds a seeded `setup.exe`. The config file (path printed at
//! start) begins with a comment that rule saves must keep.
//! Feature 010: `#/doctor` finds Put.io storage at 93%, the failed and stalled transfers,
//! a duplicate of Sintel in Shows, an orphaned `.torrent` at the top, queue row 501 for a
//! deleted file, failed queue row 403, a stale partial download in `Movies/` and an
//! unknown key in the config file. Its fixes run against the same mocks.
//! Feature 042: the Stats page's "Rejected releases" and "Magnet lookups" tiles count a
//! week of seeded release checks: fakes rejected before and on Put.io, magnets found in
//! itorrents.net or sent to Put.io, and one older reject that only 30d and longer show.
//! Flags (US6): `Movies/setup.exe` is flagged fake with a note, so it shows the badge and
//! Undo. `Shows/Example.Show.S01E01.1080p.mkv` belongs to a Sonarr item, so its Flag
//! dialog offers "Also fail and blocklist it". History → Rejected releases offers
//! "Not fake" on each row, and the Stats page's "Release reports" card downloads them.
//! Feature 040: History badges some rows Saved as copy (with the `(1)` path under the
//! name), Resumed or Overwritten; Settings has "When a file already exists" set to Keep
//! both, and `Movies` has an Overwrite rule.

use chrono::{Duration, Timelike, Utc};
use putio_sync_api::{extract_download_config, router, AppState};
use putio_sync_client::PutioApiClient;
use putio_sync_core::cleanup::EmptyFolderCleanup;
use putio_sync_core::config::{
    ArrConfig, ArrKind, ArrServer, AuthConfig, Config, ExistingFile, PrecheckOverride,
};
use putio_sync_core::rules::FolderRule;
use putio_sync_core::stall::StallTracker;
use putio_sync_db::models::{DownloadSource, HistoryAction, Origin, QueueStatus, SourceType};
use putio_sync_db::{CheckState, Database, NewArrItem, NewCheck, NewReport, ReportKind};
use putio_sync_doctor::Doctor;
use putio_sync_download::DownloadManager;
use putio_sync_telemetry::RuleDecision;
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::SystemTime;

/// The API token to sign in with; a fixed test value, not a secret
const API_TOKEN: &str = "mock-ui-token";

/// The Put.io token the mocked client sends; mockito ignores it
const PUTIO_TOKEN: &str = "mock-putio-token";

const MOVIES: i64 = 100;
const SHOWS: i64 = 200;
/// Inside Movies: open `#/putio/110` directly to see the breadcrumb walk up. Empty, so
/// the empty-folder cleanup offers to remove it.
const EXTRAS: i64 = 110;

fn file(id: i64, name: &str, size: i64, file_type: &str, parent_id: i64) -> Value {
    json!({
        "id": id,
        "name": name,
        "size": size,
        "file_type": file_type,
        "parent_id": parent_id,
        "is_folder": file_type == "FOLDER",
        "is_shared": false,
        "created_at": "2026-10-20T09:30:00",
        "content_type": if file_type == "FOLDER" { "application/x-directory" } else { "video/x-matroska" },
    })
}

fn listing(parent: Value, files: Vec<Value>) -> String {
    let total = files.len();
    json!({ "status": "OK", "files": files, "parent": parent, "total": total }).to_string()
}

fn transfer(id: i64, name: &str, status: &str, percent: i64) -> Value {
    json!({
        "id": id,
        "name": name,
        "size": 2_147_483_648_i64,
        "status": status,
        "down_speed": 3_145_728,
        "up_speed": 0,
        "downloaded": 2_147_483_648_i64 * percent / 100,
        "uploaded": 0,
        "percent_done": percent,
        "estimated_time": 420,
        "peers": 12,
        "availability": 100,
        "current_ratio": 0.0,
        "seconds_seeding": 0,
        "error_message": null,
        "created_at": "2026-10-26T08:00:00Z",
        "finished_at": null,
        "save_parent_id": 0,
        "file_id": null,
        "is_private": false,
        "type": "TORRENT",
        "tracker": "udp://tracker.opentrackr.org:1337/announce"
    })
}

/// Stuck at 97% with no complete copy in the swarm; seeded as stalled for 7 hours
const STALLED: i64 = 5;

/// What `/transfers/list` answers: one downloading, one stalled, one failed without
/// magnet, one completed and one seeding
fn transfers() -> Vec<Value> {
    let mut sintel = transfer(1, "Sintel.2010.1080p", "DOWNLOADING", 42);
    sintel["magneturi"] = json!("magnet:?xt=urn:btih:08ada5a7a6183aae1e09d831df6748d566095a10");
    let mut stalled = transfer(STALLED, "Ubuntu.24.04.Desktop.iso", "DOWNLOADING", 97);
    stalled["down_speed"] = json!(0);
    stalled["availability"] = json!(92);
    stalled["peers_sending_to_us"] = json!(0);
    stalled["save_parent_id"] = json!(MOVIES);
    stalled["magneturi"] = json!("magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567");
    // Only a failed transfer offers Retry
    let mut failed = transfer(4, "Broken.Torrent.2009", "ERROR", 12);
    failed["error_message"] = json!("Tracker unreachable");
    let mut done = transfer(6, "Big.Buck.Bunny.2008.1080p", "COMPLETED", 100);
    done["down_speed"] = json!(0);
    let mut seeding = transfer(7, "Elephants.Dream.2006.720p", "SEEDING", 100);
    seeding["down_speed"] = json!(0);
    vec![sintel, stalled, failed, done, seeding]
}

/// Persistent mocks for every Put.io call the UI can trigger
async fn mock_putio(server: &mut mockito::ServerGuard) -> Vec<mockito::Mock> {
    let root = file(0, "Your Files", 0, "FOLDER", 0);
    let movies = file(MOVIES, "Movies", 9_000_000_000, "FOLDER", 0);
    let shows = file(SHOWS, "Shows", 4_000_000_000, "FOLDER", 0);
    let readme = file(300, "README.txt", 2_048, "TEXT", 0);
    // No transfer has this name, so the doctor reports it as left over
    let mut torrent = file(301, "Old.Release.2019.torrent", 40_960, "FILE", 0);
    torrent["extension"] = json!("torrent");
    let big = file(
        101,
        "Big.Buck.Bunny.2008.2160p.mkv",
        8_589_934_592,
        "VIDEO",
        MOVIES,
    );
    let setup = file(102, "setup.exe", 52_428_800, "FILE", MOVIES);
    let extras = file(EXTRAS, "Extras", 0, "FOLDER", MOVIES);
    // Not queued, so "Download" on Movies toasts found 3, queued 1, skipped 1, blocked 1
    let sintel = file(103, "Sintel.2010.1080p.mkv", 1_288_490_189, "VIDEO", MOVIES);
    let episode = file(
        201,
        "Example.Show.S01E01.1080p.mkv",
        1_610_612_736,
        "VIDEO",
        SHOWS,
    );
    // Blocked by the Shows "Video only" rule (feature 009)
    let nfo = file(202, "Example.Show.S01E01.nfo", 4_096, "TEXT", SHOWS);
    // Same name and size as the one in Movies: a duplicate
    let sintel_copy = file(203, "Sintel.2010.1080p.mkv", 1_288_490_189, "VIDEO", SHOWS);
    // Tears of Steel's queue row is paused; the file lives outside the walked folders
    let tears = file(402, "Tears.of.Steel.2012.720p.mkv", 734_003_200, "VIDEO", 0);
    let cosmos = file(403, "Cosmos.Laundromat.2015.mkv", 524_288_000, "VIDEO", 0);

    let transfers = transfers();

    let mut json_mock = |method: &str, path: String, body: String| {
        server
            .mock(method, path.as_str())
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(body)
            .expect_at_least(0)
    };

    let mut mocks =
        vec![
        json_mock(
            "GET",
            "/files/list?parent_id=0".into(),
            listing(
                root.clone(),
                vec![movies.clone(), shows.clone(), readme.clone(), torrent.clone()],
            ),
        ),
        json_mock(
            "GET",
            format!("/files/list?parent_id={MOVIES}"),
            listing(
                movies.clone(),
                vec![extras.clone(), big.clone(), setup.clone(), sintel.clone()],
            ),
        ),
        json_mock(
            "GET",
            format!("/files/list?parent_id={EXTRAS}"),
            listing(extras.clone(), vec![]),
        ),
        json_mock(
            "GET",
            "/transfers/list".into(),
            json!({ "status": "OK", "transfers": transfers }).to_string(),
        ),
        // Retry answers with the transfer as Put.io reports it afterwards
        json_mock(
            "GET",
            "/transfers/4".into(),
            json!({ "status": "OK", "transfer": transfers[2] }).to_string(),
        ),
        // Re-add reads the transfer for its magnet and folder, then cancels and adds
        json_mock(
            "GET",
            format!("/transfers/{STALLED}"),
            json!({ "status": "OK", "transfer": transfers[1] }).to_string(),
        ),
        json_mock(
            "POST",
            "/transfers/add".into(),
            json!({ "status": "OK", "transfer": transfer(2, "New transfer", "WAITING", 0) })
                .to_string(),
        ),
        // A `.torrent` file goes through the upload endpoint, which follows the base URL
        json_mock(
            "POST",
            "/files/upload".into(),
            json!({ "status": "OK", "transfer": transfer(3, "Uploaded torrent", "WAITING", 0) })
                .to_string(),
        ),
        json_mock("POST", "/files/delete".into(), json!({ "status": "OK" }).to_string()),
        // Remove and retry
        json_mock("POST", "/transfers/cancel".into(), json!({ "status": "OK" }).to_string()),
        json_mock("POST", "/transfers/retry".into(), json!({ "status": "OK" }).to_string()),
        // Clear by status (feature 008): no `deleted_ids`, so every id sent counts as cleaned
        json_mock("POST", "/transfers/clean".into(), json!({ "status": "OK" }).to_string()),
        // The doctor (feature 010): storage at 93%
        json_mock(
            "GET",
            "/account/info".into(),
            json!({ "status": "OK", "info": { "user_id": 1, "username": "mock-user",
                "mail": "mock@example.com", "account_active": true,
                "disk": { "size": 1_000_000_000_000_i64, "used": 930_000_000_000_i64,
                    "avail": 70_000_000_000_i64 } } })
            .to_string(),
        ),
    ];
    for f in [
        &movies,
        &shows,
        &extras,
        &readme,
        &big,
        &setup,
        &sintel,
        &episode,
        &sintel_copy,
        &tears,
        &cosmos,
        &nfo,
        &torrent,
    ] {
        mocks.push(json_mock(
            "GET",
            format!("/files/{}", f["id"]),
            json!({ "status": "OK", "file": f }).to_string(),
        ));
    }

    // The Shows listing answers after 3 s, so a "sync now" (which walks every folder)
    // stays in progress long enough to see it and to try a second one
    let shows_listing = listing(
        shows.clone(),
        vec![episode.clone(), nfo.clone(), sintel_copy.clone()],
    );
    mocks.push(
        server
            .mock("GET", format!("/files/list?parent_id={SHOWS}").as_str())
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_chunked_body(move |w| {
                std::thread::sleep(std::time::Duration::from_secs(3));
                w.write_all(shows_listing.as_bytes())
            })
            .expect_at_least(0),
    );

    // Queue row 501 points at a file deleted from Put.io
    mocks.push(
        server
            .mock("GET", "/files/501")
            .with_status(404)
            .with_header("content-type", "application/json")
            .with_body(r#"{"status":"ERROR","error_type":"NotFound"}"#)
            .expect_at_least(0),
    );

    // `#/putio/999` is a folder that no longer exists: the page shows the error and Retry
    mocks.push(
        server
            .mock("GET", "/files/list?parent_id=999")
            .with_status(404)
            .with_header("content-type", "application/json")
            .with_body(r#"{"status":"ERROR","error_type":"NotFound"}"#)
            .expect_at_least(0),
    );

    let mut created = Vec::with_capacity(mocks.len());
    for mock in mocks {
        created.push(mock.create_async().await);
    }
    created
}

/// 64 history rows, two pages of 50: every fourth one a deletion, every tenth one an
/// `ubuntu` image, so the name filter has something to find
async fn seed_history(db: &Database) -> Result<(), Box<dyn std::error::Error>> {
    const NAMES: [&str; 5] = [
        "Big.Buck.Bunny.2008.1080p.mkv",
        "Sintel.2010.720p.mkv",
        "debian-12.7.0-amd64-netinst.iso",
        "Cosmos.Laundromat.2015.mkv",
        "Spring.2019.2160p.mkv",
    ];
    let history = db.history();
    for i in 0..64_i64 {
        let name = if i % 10 == 3 {
            format!("Ubuntu-24.04.{i}-desktop-amd64.iso")
        } else {
            format!("{:02}.{}", i, NAMES[i as usize % NAMES.len()])
        };
        let size = 104_857_600 * (i % 7 + 1);
        if i % 4 == 0 {
            history
                .insert(500 + i, &name, size, HistoryAction::Deleted, None, None)
                .await?;
        } else {
            let seconds = 30 + i * 5;
            let id = history
                .insert(
                    500 + i,
                    &name,
                    size,
                    HistoryAction::Downloaded,
                    Some(seconds),
                    Some(size / seconds),
                )
                .await?;
            // Feature 040: one saved under a new name, one resumed, one overwritten
            let seen = match i % 16 {
                1 => Some(("saved_as", name.replace(".mkv", " (1).mkv"))),
                2 => Some(("resumed", name.clone())),
                5 => Some(("overwritten", name.clone())),
                _ => None,
            };
            if let Some((outcome, saved)) = seen {
                let path = format!("/downloads/Movies/{saved}");
                history.set_outcome(id, Some(&path), Some(outcome)).await?;
            }
        }
    }
    Ok(())
}

/// About 120 days of downloads for the Stats page: mostly evenings, more at weekends,
/// a mix of file types, sources (two public trackers, one private, URL, unknown) and
/// origins (two watch folders, manual, Put.io)
/// A magnet held before Put.io (043), for the Queue page's "Waiting for file list". Its
/// one ask is 12 minutes out, so the mock never reaches a cache.
async fn seed_held(db: &Database) -> Result<(), Box<dyn std::error::Error>> {
    let hash = "4f1c9a3e2b7d6c5a8e9f0b1c2d3e4f5a6b7c8d9e";
    db.arr()
        .insert(&putio_sync_db::NewArrItem {
            hash,
            name: "Show.S02E05.1080p.WEB",
            category: "sonarr",
            transfer_id: None,
            putio_status: putio_sync_arr::HELD,
            total_size: 0,
        })
        .await?;
    let now = Utc::now().timestamp();
    db.held()
        .insert(&putio_sync_db::NewHeld {
            hash,
            magnet: "magnet:?xt=urn:btih:4f1c9a3e2b7d6c5a8e9f0b1c2d3e4f5a6b7c8d9e",
            source: "magnet",
            category: Some("sonarr"),
            watch_folder: None,
            check_id: None,
            last_lookup: Some("not_found"),
            held_at: now - 90,
            next_at: now + 720,
            until_at: now + 720,
        })
        .await?;
    Ok(())
}

async fn seed_stats(db: &Database) -> Result<(), Box<dyn std::error::Error>> {
    // (extension, minimum size, size spread)
    const KINDS: [(&str, i64, i64); 9] = [
        ("mkv", 700_000_000, 6_000_000_000),
        ("mkv", 1_500_000_000, 9_000_000_000),
        ("mp4", 300_000_000, 2_000_000_000),
        ("flac", 20_000_000, 60_000_000),
        ("mp3", 4_000_000, 8_000_000),
        ("zip", 50_000_000, 900_000_000),
        ("srt", 40_000, 80_000),
        ("pdf", 1_000_000, 30_000_000),
        ("", 1_000_000, 50_000_000),
    ];
    const FOLDERS: [&str; 5] = ["Movies", "TV", "Music", "Books", "(root)"];
    // A fixed linear congruential generator keeps every run the same
    let mut state: u64 = 0x5eed;
    let mut next = move |n: u64| {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (state >> 33) % n
    };
    let history = db.history();
    let sources = db.sources();
    let today = Utc::now().with_minute(0).and_then(|t| t.with_second(0));
    let today = today.ok_or("time")?;
    let mut file_id = 10_000_i64;
    for day in 0..120_i64 {
        let date = today - Duration::days(day);
        let weekend = date.format("%u").to_string().parse::<u32>()? >= 6;
        let count = next(4) + if weekend { 3 } else { 1 };
        for _ in 0..count {
            let hour = if next(10) < 7 { 18 + next(6) } else { next(24) } as i64;
            let finish = date - Duration::hours(date.hour() as i64)
                + Duration::hours(hour)
                + Duration::minutes(next(60) as i64);
            if finish > Utc::now() {
                continue;
            }
            let (ext, min, spread) = KINDS[next(KINDS.len() as u64) as usize];
            let size = min + next(spread as u64) as i64;
            let seconds = (size / (2_000_000 + next(18_000_000) as i64)).max(1);
            let name = if ext.is_empty() {
                format!("file-{file_id}")
            } else {
                format!("file-{file_id}.{ext}")
            };
            file_id += 1;
            history
                .insert_download_at(
                    file_id,
                    &name,
                    size,
                    Some(seconds),
                    Some(size / seconds),
                    finish,
                )
                .await?;

            let (source_type, tracker, private) = match next(10) {
                0..=3 => (
                    Some(SourceType::Torrent),
                    Some("tracker.opentrackr.org"),
                    false,
                ),
                4..=5 => (Some(SourceType::Torrent), Some("open.demonii.com"), false),
                6 => (
                    Some(SourceType::Torrent),
                    Some("tracker.private.example"),
                    true,
                ),
                7 => (Some(SourceType::Url), None, false),
                _ => (None, None, false),
            };
            if source_type.is_none() && next(2) == 0 {
                continue; // queued before sources were recorded: no row at all
            }
            let (origin, watch_folder) = match next(6) {
                0 | 1 => (Origin::Watch, Some("/watch/tv")),
                2 => (Origin::Watch, Some("/watch/movies")),
                3 => (Origin::Manual, None),
                _ if source_type.is_some() => (Origin::Putio, None),
                _ => (Origin::Unknown, None),
            };
            let source = DownloadSource {
                transfer_id: source_type.map(|_| 900 + next(50) as i64),
                source_type,
                tracker_host: tracker.map(str::to_string),
                is_private: private,
                origin,
                watch_folder: watch_folder.map(str::to_string),
                putio_folder: Some(FOLDERS[next(FOLDERS.len() as u64) as usize].to_string()),
            };
            sources.record(file_id, &source).await?;
        }
    }
    Ok(())
}

/// A week of release checks for the Stats page's release tiles (feature 042), with one
/// older reject that only the 30d range and longer count
async fn seed_releases(db: &Database) -> Result<(), Box<dyn std::error::Error>> {
    const GB: i64 = 1_073_741_824;
    // (days ago, name, source, stage, state, reason, lookup, cache, size, Put.io bytes)
    type Row = (
        i64,
        &'static str,
        &'static str,
        &'static str,
        CheckState,
        Option<&'static str>,
        Option<&'static str>,
        Option<&'static str>,
        i64,
        i64,
    );
    use CheckState::{Passed, Rejected};
    const ROWS: [Row; 12] = [
        (
            0,
            "Movie.2024.2160p",
            "torrent",
            "before",
            Rejected,
            Some("no_wanted"),
            None,
            None,
            2 * GB,
            0,
        ),
        (
            1,
            "Movie.2024.1080p",
            "url",
            "before",
            Rejected,
            Some("no_wanted"),
            None,
            None,
            1_500_000_000,
            0,
        ),
        (
            1,
            "Show.S02E04",
            "watch_torrent",
            "before",
            Rejected,
            Some("small_wanted"),
            None,
            None,
            40_000_000,
            0,
        ),
        (
            2,
            "Movie.2023.REPACK",
            "magnet",
            "putio",
            Rejected,
            Some("no_wanted"),
            Some("not_found"),
            None,
            1_800_000_000,
            1_800_000_000,
        ),
        (
            3,
            "Album.FLAC",
            "magnet",
            "putio",
            Passed,
            None,
            Some("timeout"),
            None,
            600_000_000,
            600_000_000,
        ),
        (
            3,
            "Show.S02E03",
            "magnet",
            "before",
            Passed,
            None,
            Some("found"),
            Some("itorrents.net"),
            900_000_000,
            0,
        ),
        (
            4,
            "Show.S02E02",
            "magnet",
            "before",
            Rejected,
            Some("no_wanted"),
            Some("found"),
            Some("itorrents.net"),
            1_200_000_000,
            0,
        ),
        (
            4,
            "Old.Movie.1999",
            "watch_magnet",
            "putio",
            Rejected,
            Some("gone"),
            Some("not_found"),
            None,
            0,
            0,
        ),
        (
            5,
            "Show.S02E01",
            "torrent",
            "before",
            Passed,
            None,
            None,
            None,
            1_100_000_000,
            0,
        ),
        (
            5,
            "Movie.2022",
            "magnet",
            "putio",
            Rejected,
            Some("small_wanted"),
            Some("wrong_hash"),
            None,
            300_000_000,
            300_000_000,
        ),
        (
            6,
            "Show.S01E10",
            "magnet",
            "before",
            Passed,
            None,
            Some("found"),
            Some("itorrents.net"),
            1_000_000_000,
            0,
        ),
        (
            20,
            "Movie.2021",
            "torrent",
            "before",
            Rejected,
            Some("invalid"),
            None,
            None,
            0,
            0,
        ),
    ];
    let releases = db.releases();
    for (days, name, source, stage, state, reason, lookup, cache, size, putio_bytes) in ROWS {
        let id = releases
            .insert_check(&NewCheck {
                name,
                source,
                stage,
                state: Some(state),
                reason,
                lookup,
                lookup_cache: cache,
                total_size: size,
                ..NewCheck::default()
            })
            .await?;
        let at = format!("-{days} days");
        sqlx::query(
            "UPDATE release_checks SET checked_at = datetime('now', ?), resolved_at = datetime('now', ?), putio_bytes = ? WHERE id = ?",
        )
        .bind(&at)
        .bind(&at)
        .bind(putio_bytes)
        .bind(id)
        .execute(db.pool())
        .await?;
        if reason.is_some_and(|r| r != "gone") {
            let types = if source.contains("magnet") {
                r#"[{"ext":"7z","count":1,"largest":900000000},{"ext":"txt","count":1,"largest":120}]"#
            } else {
                r#"[{"ext":"exe","count":1,"largest":1400000000},{"ext":"nfo","count":1,"largest":900}]"#
            };
            let report = releases
                .insert_report(&NewReport {
                    kind: ReportKind::Rejected,
                    check_id: Some(id),
                    source: Some(source),
                    stage: Some(stage),
                    name,
                    total_size: Some(size),
                    types,
                    tracker_hosts: r#"["tracker.opentrackr.org"]"#,
                    reason,
                    ..NewReport::default()
                })
                .await?;
            sqlx::query("UPDATE release_reports SET created_at = datetime('now', ?) WHERE id = ?")
                .bind(&at)
                .bind(report)
                .execute(db.pool())
                .await?;
        }
    }

    // One fake flagged by hand, and a Sonarr item the Shows episode belongs to
    releases
        .upsert_flag(&NewReport {
            kind: ReportKind::FlaggedFake,
            source: Some("putio"),
            name: "setup.exe",
            file_id: Some(102),
            total_size: Some(52_428_800),
            types: r#"[{"ext":"exe","count":1,"largest":52428800}]"#,
            file_count: Some(1),
            largest_wanted: Some(0),
            tracker_hosts: "[]",
            note: Some("An installer in a movie folder"),
            ..NewReport::default()
        })
        .await?;
    db.arr()
        .insert(&NewArrItem {
            hash: "8f14e45fceea167a5a36dedd4bea2543f1f2a3b4",
            name: "Example.Show.S01E01.1080p",
            category: "sonarr",
            transfer_id: None,
            putio_status: "COMPLETED",
            total_size: 1_610_612_736,
        })
        .await?;
    sqlx::query("UPDATE arr_items SET file_id = 201 WHERE category = 'sonarr'")
        .execute(db.pool())
        .await?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,sqlx=warn".into()),
        )
        .init();

    let port: u16 = match std::env::args().nth(1) {
        Some(arg) => arg.parse()?,
        None => 0,
    };

    let mut putio = mockito::Server::new_async().await;
    let _mocks = mock_putio(&mut putio).await;
    // Sonarr answers the Settings card's connection test on the same mock server
    let _sonarr = putio
        .mock("GET", "/api/v3/system/status")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"appName":"Sonarr","version":"4.0.15.2941"}"#)
        .create_async()
        .await;

    let dir = tempfile::tempdir()?;
    let mut config = Config {
        auth: Some(AuthConfig::with_token(PUTIO_TOKEN.to_string())),
        ..Default::default()
    };
    config.download.target_directory = dir.path().to_path_buf();
    config.download.blocked_extensions = vec!["exe".to_string()];
    config.api.auth_token = Some(API_TOKEN.to_string());
    config.arr = ArrConfig {
        enabled: true,
        password: Some("mock-arr-password".to_string()),
        servers: vec![
            ArrServer {
                kind: ArrKind::Sonarr,
                url: putio.url(),
                api_key: Some("mock-sonarr-key".to_string()),
                category: "sonarr".to_string(),
            },
            ArrServer {
                kind: ArrKind::Radarr,
                url: "http://127.0.0.1:7878".to_string(),
                api_key: None,
                category: "radarr".to_string(),
            },
        ],
        ..Default::default()
    };
    config.download.set_on_existing(ExistingFile::KeepBoth);
    config.download.rules = vec![
        FolderRule {
            path: "Shows".to_string(),
            preset: Some("video".to_string()),
            ..Default::default()
        },
        FolderRule {
            path: "Movies".to_string(),
            on_existing: Some(ExistingFile::Overwrite),
            ..Default::default()
        },
        FolderRule {
            path: "lidarr".to_string(),
            preset: Some("audio".to_string()),
            ..Default::default()
        },
    ];
    // The Release check card: one category off, one with its own size and a rule
    config.precheck.categories = [
        (
            "radarr",
            PrecheckOverride {
                min_size_mb: Some(300),
                ..Default::default()
            },
        ),
        ("lidarr", PrecheckOverride::default()),
        (
            "software",
            PrecheckOverride {
                enabled: Some(false),
                ..Default::default()
            },
        ),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();
    let config_path = dir.path().join("config.toml");
    config.save(&config_path)?;
    // A comment UI saves must keep (save_keeping_file_credentials edits in place), and an
    // unknown key for the doctor's config check
    let saved = std::fs::read_to_string(&config_path)?;
    std::fs::write(
        &config_path,
        format!("# Seeded by mock_ui: UI saves keep this comment\nsync_everything = true\n{saved}"),
    )?;

    let db = Arc::new(Database::new(dir.path().join(".putiosync.db")).await?);
    let queue = db.queue();
    let rows = [
        (
            101,
            "Big.Buck.Bunny.2008.2160p.mkv",
            8_589_934_592,
            QueueStatus::Pending,
        ),
        (
            201,
            "Example.Show.S01E01.1080p.mkv",
            1_610_612_736,
            QueueStatus::Pending,
        ),
        (
            401,
            "Elephants.Dream.2006.1080p.mkv",
            1_073_741_824,
            QueueStatus::Completed,
        ),
        (
            402,
            "Tears.of.Steel.2012.720p.mkv",
            734_003_200,
            QueueStatus::Paused,
        ),
        (
            403,
            "Cosmos.Laundromat.2015.mkv",
            524_288_000,
            QueueStatus::Failed,
        ),
        (
            501,
            "Removed.Movie.2001.mkv",
            2_147_483_648,
            QueueStatus::Pending,
        ),
    ];
    // Listed (and downloaded) highest priority first, so the first row gets the highest
    let top = rows.len() as i64;
    for (index, (file_id, name, size, status)) in rows.into_iter().enumerate() {
        queue
            .add(file_id, name, size, "http://unused", "", top - index as i64)
            .await?;
        if status != QueueStatus::Pending {
            queue.update_status(file_id, status).await?;
        }
    }

    seed_history(&db).await?;
    seed_stats(&db).await?;
    seed_releases(&db).await?;
    seed_held(&db).await?;
    seed_partial(dir.path())?;
    db.rule_decisions()
        .record(
            102,
            "setup.exe",
            "Movies",
            RuleDecision::Blocked,
            "extension exe is blocked",
            None,
        )
        .await?;

    let client = Arc::new(PutioApiClient::with_base_url(
        PUTIO_TOKEN.to_string(),
        putio.url(),
    ));
    let manager =
        DownloadManager::new(client.clone(), db.clone(), extract_download_config(&config));
    let tracker = seeded_tracker();
    let cleanup = Arc::new(EmptyFolderCleanup::from_config(client.clone(), &config));
    let doctor = Doctor::new(
        config.clone(),
        client.clone(),
        Some(db.clone()),
        cleanup.clone(),
    )
    .with_stalls(tracker.clone())
    .with_config_path(Some(config_path.clone()));
    let config_path_shown = config_path.clone();
    let state = AppState::new(
        config,
        Some(config_path),
        client,
        Some(db),
        Arc::new(manager),
        None,
    )
    .with_stall_tracker(tracker)
    .with_cleanup(cleanup)
    .with_doctor(Arc::new(doctor));
    if let Some(arr) = &state.arr {
        arr.service
            .contacts()
            .record(Some("Sonarr/4.0.15.2941 (ubuntu 22.04)"));
    }

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
    let addr = listener.local_addr()?;
    println!("Open http://{addr}/ and sign in with token: {API_TOKEN}");
    println!("Config file: {}", config_path_shown.display());
    println!("Mocked Put.io at {}; press Ctrl+C to stop.", putio.url());

    axum::serve(
        listener,
        router(state).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await?;
    Ok(())
}

/// A partial download in `Movies/` that no queue row owns, last written 3 hours ago
fn seed_partial(root: &std::path::Path) -> std::io::Result<()> {
    let dir = root.join("Movies");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(".Lost.Download.2020.mkv.downloading");
    std::fs::write(&path, vec![0u8; 4096])?;
    std::fs::File::options()
        .write(true)
        .open(&path)?
        .set_modified(SystemTime::now() - std::time::Duration::from_secs(3 * 3600))
}

/// A tracker that saw the transfer list 7 hours ago and now, so transfer 5 shows as
/// stalled without waiting for the daemon's check
fn seeded_tracker() -> Arc<StallTracker> {
    let seen: Vec<putio_sync_client::Transfer> = transfers()
        .into_iter()
        .map(|t| serde_json::from_value(t).expect("mock transfer parses"))
        .collect();
    let tracker = StallTracker::default();
    let now = SystemTime::now();
    // Sintel had fewer bytes then, so only transfer 5 stood still
    let mut earlier = seen.clone();
    earlier[0].downloaded = earlier[0].downloaded.map(|bytes| bytes - 1_000_000);
    tracker.observe(&earlier, now - std::time::Duration::from_secs(7 * 3600));
    tracker.observe(&seen, now);
    Arc::new(tracker)
}
