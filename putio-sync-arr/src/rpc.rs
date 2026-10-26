//! The Transmission RPC subset Radarr and Sonarr use (`contracts/transmission-rpc.md`)

use base64::Engine;
use putio_sync_telemetry::arr as metrics;
use putio_sync_telemetry::labels::{ArrMethod, ArrOutcome};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use tracing::warn;

use crate::service::{AddOutcome, AddSource, ArrError, ArrService, ItemId};

/// The version string `session-get` reports
pub const VERSION: &str = "3.00 (putio-sync)";

/// One RPC request body
#[derive(Debug, Deserialize)]
pub struct Request {
    /// e.g. `torrent-get`
    pub method: String,
    /// Method arguments
    #[serde(default)]
    pub arguments: Option<Map<String, Value>>,
    /// Echoed back
    #[serde(default)]
    pub tag: Option<Value>,
}

/// The reply to a body that isn't a valid request
pub fn invalid_request() -> Value {
    json!({ "result": "invalid request" })
}

/// Handle one request and build the reply
pub async fn handle(service: &ArrService, request: Request) -> Value {
    let method = ArrMethod::from_method(&request.method);
    let args = request.arguments.unwrap_or_default();
    let result = dispatch(service, &request.method, &args).await;
    let (outcome, mut reply) = match result {
        Ok(arguments) => (
            ArrOutcome::Ok,
            json!({ "result": "success", "arguments": arguments }),
        ),
        Err(text) => (
            ArrOutcome::Error,
            json!({ "result": text, "arguments": {} }),
        ),
    };
    metrics::rpc_request(method, outcome);
    if let Some(tag) = request.tag {
        reply["tag"] = tag;
    }
    reply
}

async fn dispatch(
    service: &ArrService,
    method: &str,
    args: &Map<String, Value>,
) -> Result<Value, String> {
    match method {
        "session-get" => Ok(session(service)),
        "torrent-add" => torrent_add(service, args).await,
        "torrent-get" => {
            service.refresh_if_stale().await;
            let ids = parse_ids(args.get("ids"));
            let torrents = service
                .torrents(ids.as_deref())
                .await
                .map_err(|e| internal(&e))?;
            Ok(json!({ "torrents": torrents }))
        }
        "torrent-remove" => {
            let delete_local = args
                .get("delete-local-data")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let ids = parse_ids(args.get("ids")).unwrap_or_default();
            let hashes = service.resolve(&ids).await.map_err(|e| internal(&e))?;
            for hash in hashes {
                service
                    .remove(&hash, delete_local)
                    .await
                    .map_err(|e| internal(&e))?;
            }
            Ok(json!({}))
        }
        "session-stats" => Ok(json!({
            "activeTorrentCount": 0, "pausedTorrentCount": 0, "torrentCount": 0,
            "downloadSpeed": 0, "uploadSpeed": 0,
        })),
        "torrent-set" | "queue-move-top" | "queue-move-bottom" | "torrent-start"
        | "torrent-start-now" | "torrent-stop" | "session-set" => Ok(json!({})),
        _ => Err("method not supported".to_string()),
    }
}

fn internal(e: &ArrError) -> String {
    warn!(error = %e, "arr RPC failed");
    e.to_string()
}

fn session(service: &ArrService) -> Value {
    json!({
        "version": VERSION,
        "rpc-version": 17,
        "rpc-version-minimum": 14,
        "download-dir": service.download_dir(),
        "seedRatioLimit": 0,
        "seedRatioLimited": false,
        "idle-seeding-limit": 0,
        "idle-seeding-limit-enabled": true,
    })
}

async fn torrent_add(service: &ArrService, args: &Map<String, Value>) -> Result<Value, String> {
    let source = if let Some(meta) = args.get("metainfo").and_then(Value::as_str) {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(meta.trim())
            .map_err(|_| ArrError::InvalidTorrent.to_string())?;
        AddSource::Metainfo(bytes)
    } else if let Some(filename) = args.get("filename").and_then(Value::as_str) {
        let filename = filename.trim();
        if filename.starts_with("magnet:") {
            AddSource::Magnet(filename.to_string())
        } else {
            AddSource::Url(filename.to_string())
        }
    } else {
        return Err("torrent-add needs filename or metainfo".to_string());
    };
    let download_dir = args.get("download-dir").and_then(Value::as_str);

    let outcome = service.add(source, download_dir).await.map_err(|e| {
        if matches!(e, ArrError::Internal(_)) {
            internal(&e)
        } else {
            e.to_string()
        }
    })?;
    let (key, item) = match outcome {
        AddOutcome::Added(item) => ("torrent-added", item),
        AddOutcome::Duplicate(item) => ("torrent-duplicate", item),
    };
    Ok(json!({ key: { "id": item.id, "name": item.name, "hashString": item.hash } }))
}

/// `ids`: a number, a hash, or a list of them. `None` means every item; the
/// `recently-active` form also lists every item.
fn parse_ids(ids: Option<&Value>) -> Option<Vec<ItemId>> {
    fn one(v: &Value) -> Option<ItemId> {
        match v {
            Value::Number(n) => n.as_i64().map(ItemId::Id),
            Value::String(s) => Some(ItemId::Hash(s.to_string())),
            _ => None,
        }
    }
    match ids? {
        Value::Array(list) => Some(list.iter().filter_map(one).collect()),
        Value::String(s) if s == "recently-active" => None,
        v => Some(one(v).into_iter().collect()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_forms() {
        assert_eq!(parse_ids(None), None);
        assert_eq!(parse_ids(Some(&json!("recently-active"))), None);
        assert_eq!(parse_ids(Some(&json!(3))), Some(vec![ItemId::Id(3)]));
        assert_eq!(
            parse_ids(Some(&json!(["ABC", 4, null]))),
            Some(vec![ItemId::Hash("ABC".into()), ItemId::Id(4)])
        );
    }
}
