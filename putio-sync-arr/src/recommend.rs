//! Settings in the *arr that work best with putio-sync (feature 049, R5): Completed Download
//! Handling, and Import Extra Files with `srt`. Shown and turned on one at a time, never by
//! default.

use serde::Serialize;
use serde_json::{json, Value};
use tracing::info;

use crate::arr_api::{ArrApi, ArrApiError};

/// The settings, by name
pub const NAMES: &[&str] = &["completed_download_handling", "import_extra_files"];

/// data-model "Recommendation"
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Recommendation {
    /// One of [`NAMES`]
    pub name: &'static str,
    /// Already as recommended
    pub on: bool,
}

const DOWNLOAD_CLIENT: &str = "downloadclient";
const MEDIA_MANAGEMENT: &str = "mediamanagement";

fn has_srt(extensions: &str) -> bool {
    extensions
        .split(',')
        .any(|e| e.trim().trim_start_matches('.').eq_ignore_ascii_case("srt"))
}

fn handling(body: &Value) -> Option<bool> {
    body.get("enableCompletedDownloadHandling")
        .and_then(Value::as_bool)
}

fn extra_files(body: &Value) -> Option<bool> {
    let on = body.get("importExtraFiles").and_then(Value::as_bool)?;
    let ext = body
        .get("extraFileExtensions")
        .and_then(Value::as_str)
        .unwrap_or("");
    Some(on && has_srt(ext))
}

/// Both settings as the *arr has them. One it doesn't have is left out.
pub async fn read(api: &ArrApi) -> Result<Vec<Recommendation>, ArrApiError> {
    let mut out = Vec::new();
    if let Some(on) = handling(&api.config(DOWNLOAD_CLIENT).await?) {
        out.push(Recommendation { name: NAMES[0], on });
    }
    if let Some(on) = extra_files(&api.config(MEDIA_MANAGEMENT).await?) {
        out.push(Recommendation { name: NAMES[1], on });
    }
    Ok(out)
}

/// Turn one on: the whole body is sent back with that one change. `false` when the *arr
/// doesn't have the setting.
pub async fn turn_on(api: &ArrApi, name: &str) -> Result<bool, ArrApiError> {
    let (section, mut body) = match name {
        "completed_download_handling" => (DOWNLOAD_CLIENT, api.config(DOWNLOAD_CLIENT).await?),
        _ => (MEDIA_MANAGEMENT, api.config(MEDIA_MANAGEMENT).await?),
    };
    if section == DOWNLOAD_CLIENT {
        if handling(&body).is_none() {
            return Ok(false);
        }
        body["enableCompletedDownloadHandling"] = json!(true);
    } else {
        if extra_files(&body).is_none() {
            return Ok(false);
        }
        body["importExtraFiles"] = json!(true);
        let ext = body
            .get("extraFileExtensions")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if !has_srt(&ext) {
            let joined = if ext.is_empty() {
                "srt".to_string()
            } else {
                format!("{},srt", ext.trim_end_matches(','))
            };
            body["extraFileExtensions"] = json!(joined);
        }
    }
    api.put_config(section, &body).await?;
    info!(
        kind = api.kind().as_str(),
        setting = name,
        "arr setting turned on"
    );
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Matcher;
    use putio_sync_core::config::ArrKind;

    fn api(server: &mockito::Server) -> ArrApi {
        ArrApi::new(
            ArrKind::Radarr,
            &server.url(),
            "key-123",
            reqwest::Client::new(),
        )
    }

    #[test]
    fn srt_is_found_in_the_list() {
        assert!(has_srt("nfo, .SRT"));
        assert!(!has_srt("nfo,srtx"));
        assert!(!has_srt(""));
    }

    #[tokio::test]
    async fn a_missing_setting_is_left_out_and_srt_is_appended() {
        let mut server = mockito::Server::new_async().await;
        // Newer versions removed the Completed Download Handling switch
        server
            .mock("GET", "/api/v3/config/downloadclient")
            .with_body(r#"{"id": 1, "autoRedownloadFailed": true}"#)
            .create_async()
            .await;
        server
            .mock("GET", "/api/v3/config/mediamanagement")
            .with_body(
                r#"{"id": 3, "importExtraFiles": false, "extraFileExtensions": "nfo",
                    "recycleBin": "/trash"}"#,
            )
            .create_async()
            .await;
        let put = server
            .mock("PUT", "/api/v3/config/mediamanagement/3")
            .match_body(Matcher::Json(json!({"id": 3, "importExtraFiles": true,
                "extraFileExtensions": "nfo,srt", "recycleBin": "/trash"})))
            .with_status(202)
            .with_body("{}")
            .create_async()
            .await;
        let api = api(&server);
        assert_eq!(
            read(&api).await.unwrap(),
            vec![Recommendation {
                name: "import_extra_files",
                on: false
            }]
        );
        assert!(!turn_on(&api, "completed_download_handling").await.unwrap());
        assert!(turn_on(&api, "import_extra_files").await.unwrap());
        put.assert_async().await;
    }

    #[tokio::test]
    async fn completed_download_handling_is_turned_on_with_the_whole_body() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/api/v3/config/downloadclient")
            .with_body(r#"{"id": 1, "enableCompletedDownloadHandling": false, "x": 5}"#)
            .create_async()
            .await;
        let put = server
            .mock("PUT", "/api/v3/config/downloadclient/1")
            .match_body(Matcher::Json(
                json!({"id": 1, "enableCompletedDownloadHandling": true, "x": 5}),
            ))
            .with_status(202)
            .with_body("{}")
            .create_async()
            .await;
        assert!(turn_on(&api(&server), "completed_download_handling")
            .await
            .unwrap());
        put.assert_async().await;
    }
}
