//! The objects of the qBittorrent Web API v2 (feature 067), built from the same items the
//! Transmission view (011) reads. A torrent is a Put.io transfer plus its download; what
//! Put.io has no value for (peers, seeds, ratios, trackers) gets the fixed answers below,
//! which `specs/067-qbittorrent-api/spec.md` lists.

use crate::service::Snapshot;
use crate::view;
use putio_sync_db::ArrState;
use serde_json::{json, Map, Value};
use std::path::Path;

/// `app/version`: a qBittorrent 4.6 version, so tools use the 4.x names (`paused`,
/// `pausedDL`, `pausedUP`). The 5.x names (`stop`, `start`, `stopped`) are accepted too.
pub const VERSION: &str = "v4.6.7";

/// `app/webapiVersion`, the one qBittorrent 4.6 reports
pub const WEBAPI_VERSION: &str = "2.9.3";

/// qBittorrent's ETA for "unknown"
pub const ETA_UNKNOWN: i64 = 8_640_000;

/// The one tracker each torrent reports: Put.io does the announcing
pub const TRACKER: &str = "https://put.io/";

/// Failed on Put.io, or nothing in it may be downloaded
pub const STATE_ERROR: &str = "error";
/// Waiting in Put.io's queue, or held before Put.io (043)
pub const STATE_QUEUED: &str = "queuedDL";
/// Paused in putio-sync (039)
pub const STATE_PAUSED: &str = "pausedDL";
/// On Put.io or coming down, and while the processing pipeline (038) still runs
pub const STATE_DOWNLOADING: &str = "downloading";
/// Every file is on disk. Nothing seeds from here, so it reads as finished seeding.
pub const STATE_DONE: &str = "pausedUP";

/// Whether the pipeline gate applies: only to items with files on disk
fn gated(snap: &Snapshot) -> bool {
    snap.gated && matches!(snap.item.state, ArrState::Local | ArrState::Done)
}

/// The qBittorrent state of an item
pub fn state(snap: &Snapshot) -> &'static str {
    let item = &snap.item;
    if gated(snap) {
        return STATE_DOWNLOADING;
    }
    match item.state {
        ArrState::Failed => STATE_ERROR,
        ArrState::Done => STATE_DONE,
        ArrState::Putio
            if matches!(item.putio_status.as_deref(), Some("IN_QUEUE" | crate::HELD)) =>
        {
            STATE_QUEUED
        }
        ArrState::Putio | ArrState::Local if snap.local.paused => STATE_PAUSED,
        ArrState::Putio | ArrState::Local => STATE_DOWNLOADING,
    }
}

/// Whether a torrent in `state` passes the `filter` of `torrents/info`. Nothing seeds or
/// stalls here, so `seeding` and the `stalled` filters match nothing.
pub fn matches_filter(state: &str, speed: u64, filter: &str) -> bool {
    let done = state == STATE_DONE;
    let paused = state == STATE_PAUSED || done;
    match filter {
        "" | "all" => true,
        "downloading" => !done,
        "completed" => done,
        "paused" | "stopped" => paused,
        "resumed" | "running" => !paused,
        "active" => state == STATE_DOWNLOADING && speed > 0,
        "inactive" => !(state == STATE_DOWNLOADING && speed > 0),
        "errored" => state == STATE_ERROR,
        "seeding"
        | "stalled"
        | "stalled_uploading"
        | "stalled_downloading"
        | "checking"
        | "moving" => false,
        _ => true,
    }
}

/// Where the item's files are (or will be): `<save_path>/<name on disk>`
fn content_path(snap: &Snapshot, save_path: &str) -> String {
    let name = snap.item.output_name.as_deref().unwrap_or(&snap.item.name);
    format!("{save_path}/{name}")
}

/// The torrent object of `torrents/info` and `sync/maindata`
pub fn torrent(snap: &Snapshot, target: &Path) -> Value {
    let item = &snap.item;
    let gated = gated(snap);
    let (left, progress) = view::progress(item, &snap.local, gated);
    let state = state(snap);
    let done = state == STATE_DONE;
    let save_path = view::download_dir(target, &item.category);
    let eta = match view::eta(item, &snap.local) {
        eta if eta >= 0 && !gated && !done => eta,
        _ => ETA_UNKNOWN,
    };
    let completed = (item.total_size - left).max(0);
    json!({
        "added_on": snap.added_on,
        "amount_left": left,
        "auto_tmm": false,
        "availability": -1,
        "category": item.category,
        "completed": completed,
        "completion_on": if done { snap.changed_on } else { -1 },
        "content_path": content_path(snap, &save_path),
        "dl_limit": -1,
        "dlspeed": snap.local.speed_bps,
        "download_path": "",
        "downloaded": completed,
        "downloaded_session": completed,
        "eta": eta,
        "f_l_piece_prio": false,
        "force_start": false,
        "hash": item.hash,
        "inactive_seeding_time_limit": -2,
        "infohash_v1": item.hash,
        "infohash_v2": "",
        "last_activity": snap.changed_on,
        "magnet_uri": format!("magnet:?xt=urn:btih:{}", item.hash),
        "max_inactive_seeding_time": -1,
        "max_ratio": 0,
        "max_seeding_time": -1,
        "name": item.name,
        "num_complete": 0,
        "num_incomplete": 0,
        "num_leechs": 0,
        "num_seeds": 0,
        "priority": if done { 0 } else { item.id },
        "progress": progress,
        // 0 of a limit of 0: the limit is reached at once, so a finished item may be removed
        "ratio": 0,
        "ratio_limit": 0,
        "save_path": save_path,
        "seeding_time": 0,
        "seeding_time_limit": -2,
        "seen_complete": if done { snap.changed_on } else { -1 },
        "seq_dl": false,
        "size": item.total_size,
        "state": state,
        "super_seeding": false,
        "tags": "",
        "time_active": 0,
        "total_size": item.total_size,
        "tracker": TRACKER,
        "trackers_count": 1,
        "up_limit": -1,
        "uploaded": 0,
        "uploaded_session": 0,
        "upspeed": 0,
    })
}

/// `torrents/properties`
pub fn properties(snap: &Snapshot, target: &Path) -> Value {
    let item = &snap.item;
    let torrent = torrent(snap, target);
    let done = torrent["state"] == STATE_DONE;
    json!({
        "addition_date": snap.added_on,
        "comment": if item.state == ArrState::Failed {
            item.error.clone().unwrap_or_default()
        } else {
            String::new()
        },
        "completion_date": if done { snap.changed_on } else { -1 },
        "created_by": "",
        "creation_date": -1,
        "dl_limit": -1,
        "dl_speed": snap.local.speed_bps,
        "dl_speed_avg": 0,
        "download_path": "",
        "eta": torrent["eta"],
        "hash": item.hash,
        "infohash_v1": item.hash,
        "infohash_v2": "",
        "is_private": false,
        "last_seen": -1,
        "name": item.name,
        "nb_connections": 0,
        "nb_connections_limit": -1,
        "peers": 0,
        "peers_total": 0,
        "piece_size": 0,
        "pieces_have": 0,
        "pieces_num": 0,
        "reannounce": 0,
        "save_path": torrent["save_path"],
        "seeding_time": 0,
        "seeds": 0,
        "seeds_total": 0,
        "share_ratio": 0,
        "time_elapsed": 0,
        "total_downloaded": torrent["downloaded"],
        "total_downloaded_session": torrent["downloaded"],
        "total_size": item.total_size,
        "total_uploaded": 0,
        "total_uploaded_session": 0,
        "total_wasted": 0,
        "up_limit": -1,
        "up_speed": 0,
        "up_speed_avg": 0,
    })
}

/// `torrents/files`: the files on disk when there are any, else one entry for the whole
/// item, since Put.io lists a transfer's files only once it finishes
pub fn files(snap: &Snapshot, on_disk: &[(String, u64)]) -> Value {
    let done = state(snap) == STATE_DONE;
    let (_, progress) = view::progress(&snap.item, &snap.local, gated(snap));
    let entry = |index: usize, name: &str, size: u64, progress: f64| {
        json!({
            "index": index,
            "name": name,
            "size": size,
            "progress": progress,
            "priority": 1,
            "is_seed": false,
            "piece_range": [0, 0],
            "availability": -1,
        })
    };
    if on_disk.is_empty() {
        let name = snap.item.output_name.as_deref().unwrap_or(&snap.item.name);
        return json!([entry(0, name, snap.item.total_size.max(0) as u64, progress)]);
    }
    Value::Array(
        on_disk
            .iter()
            .enumerate()
            .map(|(i, (name, size))| entry(i, name, *size, if done { 1.0 } else { progress }))
            .collect(),
    )
}

/// `torrents/trackers`: one working tracker, so a tool that waits for an announce (autobrr's
/// reannounce) goes on
pub fn trackers() -> Value {
    json!([{
        "url": TRACKER,
        "status": 2,
        "tier": 0,
        "num_peers": 0,
        "num_seeds": 0,
        "num_leeches": 0,
        "num_downloaded": 0,
        "msg": "",
    }])
}

/// `torrents/categories`: each category with its folder
pub fn categories(names: &[String], target: &Path) -> Value {
    let mut map = Map::new();
    for name in names {
        map.insert(
            name.clone(),
            json!({ "name": name, "savePath": view::download_dir(target, name) }),
        );
    }
    Value::Object(map)
}

/// `transfer/info`, also the base of `server_state` in `sync/maindata`
pub fn transfer_info(snaps: &[Snapshot]) -> Value {
    let speed: u64 = snaps.iter().map(|s| s.local.speed_bps).sum();
    let data: i64 = snaps.iter().map(|s| s.local.completed_bytes.max(0)).sum();
    json!({
        "connection_status": "connected",
        "dht_nodes": 0,
        "dl_info_data": data,
        "dl_info_speed": speed,
        "dl_rate_limit": 0,
        "up_info_data": 0,
        "up_info_speed": 0,
        "up_rate_limit": 0,
    })
}

/// `app/preferences`: the download folder and the fixed settings. The share limit is on at
/// a ratio of 0 with "pause" as its action, which is what a finished item looks like here.
pub fn preferences(save_path: &str, port: u16) -> Value {
    json!({
        "save_path": save_path,
        "temp_path": "",
        "temp_path_enabled": false,
        "auto_tmm_enabled": false,
        "create_subfolder_enabled": true,
        "torrent_content_layout": "Original",
        "start_paused_enabled": false,
        "dht": true,
        "pex": true,
        "lsd": true,
        "queueing_enabled": true,
        "max_active_downloads": -1,
        "max_active_torrents": -1,
        "max_active_uploads": -1,
        "max_ratio_enabled": true,
        "max_ratio": 0,
        "max_ratio_act": 0,
        "max_seeding_time_enabled": false,
        "max_seeding_time": -1,
        "max_inactive_seeding_time_enabled": false,
        "max_inactive_seeding_time": -1,
        "dl_limit": 0,
        "up_limit": 0,
        "listen_port": 0,
        "web_ui_port": port,
        "locale": "en",
    })
}

/// `app/buildInfo`: there is no Qt or libtorrent here
pub fn build_info() -> Value {
    json!({
        "qt": "0.0.0",
        "libtorrent": "0.0.0.0",
        "boost": "0.0.0",
        "openssl": "0.0.0",
        "zlib": "0.0.0",
        "bitness": 64,
        "platform": "putio-sync",
    })
}

/// `sync/maindata`: always a full update, so a client never has to merge
pub fn maindata(rid: i64, snaps: &[Snapshot], categories: &[String], target: &Path) -> Value {
    let torrents: Map<String, Value> = snaps
        .iter()
        .map(|s| (s.item.hash.clone(), torrent(s, target)))
        .collect();
    let mut server_state = transfer_info(snaps);
    if let Value::Object(state) = &mut server_state {
        state.insert("queueing".into(), json!(true));
        state.insert("use_alt_speed_limits".into(), json!(false));
        state.insert("refresh_interval".into(), json!(1500));
        state.insert("free_space_on_disk".into(), json!(0));
        state.insert("alltime_dl".into(), json!(0));
        state.insert("alltime_ul".into(), json!(0));
        state.insert("global_ratio".into(), json!("0"));
        state.insert("total_peer_connections".into(), json!(0));
    }
    json!({
        "rid": rid.max(0) + 1,
        "full_update": true,
        "torrents": torrents,
        "torrents_removed": [],
        "categories": self::categories(categories, target),
        "categories_removed": [],
        "tags": [],
        "tags_removed": [],
        "trackers": { TRACKER: snaps.iter().map(|s| s.item.hash.clone()).collect::<Vec<_>>() },
        "server_state": server_state,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::LocalProgress;
    use putio_sync_db::{ArrItem, BlocklistState};

    fn snap(state: ArrState, putio_status: &str, percent: i64) -> Snapshot {
        Snapshot {
            item: ArrItem {
                id: 3,
                hash: "c12fe1c06bba254a9dc9f519b335aa7c1367a88a".into(),
                name: "Show.S01E01".into(),
                category: "sonarr".into(),
                transfer_id: Some(9),
                file_id: None,
                output_name: None,
                total_size: 1000,
                putio_status: Some(putio_status.into()),
                putio_percent: percent,
                putio_eta: Some(30),
                state,
                error: None,
                blocklist: BlocklistState::None,
                blocklist_attempts: 0,
            },
            local: LocalProgress {
                completed_bytes: 0,
                file_count: 0,
                speed_bps: 0,
                paused: false,
            },
            gated: false,
            added_on: 1_700_000_000,
            changed_on: 1_700_000_100,
        }
    }

    #[test]
    fn states_follow_the_transmission_view() {
        assert_eq!(state(&snap(ArrState::Putio, "IN_QUEUE", 0)), STATE_QUEUED);
        assert_eq!(state(&snap(ArrState::Putio, crate::HELD, 0)), STATE_QUEUED);
        assert_eq!(
            state(&snap(ArrState::Putio, "DOWNLOADING", 50)),
            STATE_DOWNLOADING
        );
        assert_eq!(
            state(&snap(ArrState::Local, "COMPLETED", 100)),
            STATE_DOWNLOADING
        );
        assert_eq!(state(&snap(ArrState::Done, "COMPLETED", 100)), STATE_DONE);
        assert_eq!(state(&snap(ArrState::Failed, "ERROR", 0)), STATE_ERROR);

        let mut paused = snap(ArrState::Local, "COMPLETED", 100);
        paused.local.paused = true;
        assert_eq!(state(&paused), STATE_PAUSED);
    }

    #[test]
    fn a_gated_item_is_not_complete() {
        let mut s = snap(ArrState::Done, "COMPLETED", 100);
        s.gated = true;
        let t = torrent(&s, Path::new("/downloads"));
        assert_eq!(t["state"], STATE_DOWNLOADING);
        assert_eq!(t["progress"], 0.99);
        assert_eq!(t["completion_on"], -1);
        assert_eq!(t["eta"], ETA_UNKNOWN);
    }

    #[test]
    fn progress_counts_put_io_as_the_first_half() {
        let t = torrent(
            &snap(ArrState::Putio, "DOWNLOADING", 50),
            Path::new("/downloads/"),
        );
        assert_eq!(t["progress"], 0.25);
        assert_eq!(t["amount_left"], 750);
        assert_eq!(t["completed"], 250);
        assert_eq!(t["eta"], 30);
        assert_eq!(t["save_path"], "/downloads/sonarr");
        assert_eq!(t["content_path"], "/downloads/sonarr/Show.S01E01");
        assert_eq!(t["category"], "sonarr");
        assert_eq!(t["added_on"], 1_700_000_000);
    }

    #[test]
    fn a_done_item_has_reached_its_share_limit() {
        let mut s = snap(ArrState::Done, "COMPLETED", 100);
        s.item.output_name = Some("Show.S01E01.mkv".into());
        let t = torrent(&s, Path::new("/downloads"));
        assert_eq!(t["state"], STATE_DONE);
        assert_eq!(t["progress"], 1.0);
        assert_eq!(t["amount_left"], 0);
        assert_eq!(t["content_path"], "/downloads/sonarr/Show.S01E01.mkv");
        assert_eq!(t["completion_on"], 1_700_000_100);
        // Radarr and Sonarr remove a paused upload once ratio >= ratio_limit >= 0
        assert_eq!(t["ratio"], 0);
        assert_eq!(t["ratio_limit"], 0);
    }

    #[test]
    fn a_failed_item_reports_its_error() {
        let mut s = snap(ArrState::Failed, "ERROR", 0);
        s.item.error = Some("Put.io: no peers".into());
        assert_eq!(torrent(&s, Path::new("/d"))["state"], STATE_ERROR);
        assert_eq!(
            properties(&s, Path::new("/d"))["comment"],
            "Put.io: no peers"
        );
    }

    #[test]
    fn filters() {
        assert!(matches_filter(STATE_DOWNLOADING, 0, "downloading"));
        assert!(matches_filter(STATE_QUEUED, 0, "downloading"));
        assert!(!matches_filter(STATE_DONE, 0, "downloading"));
        assert!(matches_filter(STATE_DONE, 0, "completed"));
        assert!(matches_filter(STATE_DONE, 0, "paused"));
        assert!(matches_filter(STATE_PAUSED, 0, "stopped"));
        assert!(!matches_filter(STATE_DOWNLOADING, 0, "paused"));
        assert!(matches_filter(STATE_DOWNLOADING, 5, "active"));
        assert!(!matches_filter(STATE_DOWNLOADING, 0, "active"));
        assert!(matches_filter(STATE_ERROR, 0, "errored"));
        assert!(!matches_filter(STATE_DONE, 0, "seeding"));
        assert!(matches_filter(STATE_DONE, 0, "all"));
    }

    #[test]
    fn files_fall_back_to_one_entry() {
        let s = snap(ArrState::Putio, "DOWNLOADING", 50);
        let one = files(&s, &[]);
        assert_eq!(one[0]["name"], "Show.S01E01");
        assert_eq!(one[0]["size"], 1000);
        assert_eq!(one[0]["progress"], 0.25);

        let mut done = snap(ArrState::Done, "COMPLETED", 100);
        done.item.output_name = Some("Show".into());
        let listed = files(
            &done,
            &[
                ("Show/a.mkv".to_string(), 600),
                ("Show/b.srt".to_string(), 4),
            ],
        );
        assert_eq!(listed.as_array().unwrap().len(), 2);
        assert_eq!(listed[1]["name"], "Show/b.srt");
        assert_eq!(listed[1]["index"], 1);
        assert_eq!(listed[0]["progress"], 1.0);
    }

    #[test]
    fn maindata_is_a_full_update() {
        let snaps = [snap(ArrState::Putio, "DOWNLOADING", 50)];
        let data = maindata(4, &snaps, &["sonarr".to_string()], Path::new("/d"));
        assert_eq!(data["rid"], 5);
        assert_eq!(data["full_update"], true);
        assert_eq!(
            data["torrents"]["c12fe1c06bba254a9dc9f519b335aa7c1367a88a"]["state"],
            STATE_DOWNLOADING
        );
        assert_eq!(data["categories"]["sonarr"]["savePath"], "/d/sonarr");
        assert_eq!(data["server_state"]["connection_status"], "connected");
    }
}
