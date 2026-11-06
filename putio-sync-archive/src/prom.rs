//! A small Prometheus HTTP API client: the samples of one UTC day, as stored (R9)

use crate::error::{ArchiveError, ArchiveResult};
use chrono::{NaiveDate, NaiveTime};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::time::Duration;

/// The 5-minute series, as in contracts/archive-cli.md `archive metrics`
const SERIES: &str = r#"__name__=~"putio_sync:.+:[a-z]*5m",app="putio-sync""#;

/// One stored sample
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    /// Milliseconds since the Unix epoch
    pub ts_ms: i64,
    pub value: f64,
}

/// One series with its samples in time order
#[derive(Debug, Clone, PartialEq)]
pub struct Series {
    pub name: String,
    /// Labels without `__name__`
    pub labels: BTreeMap<String, String>,
    pub samples: Vec<Sample>,
}

/// Client for one Prometheus server
pub struct PromClient {
    base: String,
    http: reqwest::Client,
}

#[derive(Deserialize)]
struct Reply {
    status: String,
    error: Option<String>,
    data: Option<Data>,
}

#[derive(Deserialize)]
struct Data {
    result: Vec<Item>,
}

#[derive(Deserialize)]
struct Item {
    metric: BTreeMap<String, String>,
    /// Range selector result (matrix)
    #[serde(default)]
    values: Vec<(f64, String)>,
}

impl PromClient {
    /// A client for the server at `url`, for example `http://192.168.1.60:9090`
    pub fn new(url: &str) -> ArchiveResult<Self> {
        let parsed = reqwest::Url::parse(url)
            .map_err(|e| ArchiveError::args("invalid Prometheus URL", e))?;
        if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
            return Err(ArchiveError::args(
                "invalid Prometheus URL",
                "expected http://host:port",
            ));
        }
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|e| ArchiveError::args("cannot start the HTTP client", e))?;
        Ok(Self {
            base: url.trim_end_matches('/').to_string(),
            http,
        })
    }

    /// The `instance` to archive: `wanted` when given, else the only `instance` with
    /// `app="putio-sync"`. Several is an argument error; none is a Prometheus error.
    pub async fn instance(&self, wanted: Option<&str>) -> ArchiveResult<String> {
        if let Some(instance) = wanted {
            return Ok(instance.to_string());
        }
        let items = self
            .query(r#"count by (instance) (up{app="putio-sync"})"#, None)
            .await?;
        let mut found: Vec<String> = items
            .into_iter()
            .filter_map(|item| item.metric.get("instance").cloned())
            .collect();
        found.sort();
        found.dedup();
        match found.len() {
            0 => Err(ArchiveError::prometheus(
                "no instance found",
                "Prometheus has no series up{app=\"putio-sync\"}",
            )),
            1 => Ok(found.remove(0)),
            _ => Err(ArchiveError::args(
                "several putio-sync instances found, choose one with --instance",
                found.join(", "),
            )),
        }
    }

    /// The 5-minute series of `instance` for the UTC `day`: one instant query of a
    /// one-day range selector at the end of the day, keeping samples in `[day, day+1)`.
    /// Series left without samples are dropped.
    pub async fn fetch_day(&self, instance: &str, day: NaiveDate) -> ArchiveResult<Vec<Series>> {
        let start_ms = day.and_time(NaiveTime::MIN).and_utc().timestamp_millis();
        let end = day + chrono::Duration::days(1);
        let end_ms = start_ms + 86_400_000;
        let instance = instance.replace('\\', "\\\\").replace('"', "\\\"");
        let selector = format!(r#"{{{SERIES},instance="{instance}"}}[1d]"#);
        let at = format!("{end}T00:00:00Z");

        let mut out = Vec::new();
        for mut item in self.query(&selector, Some(&at)).await? {
            let name = item.metric.remove("__name__").unwrap_or_default();
            let mut samples = Vec::new();
            for (ts, value) in item.values {
                let ts_ms = (ts * 1000.0).round() as i64;
                if ts_ms < start_ms || ts_ms >= end_ms {
                    continue;
                }
                let value = value.parse::<f64>().map_err(|_| {
                    ArchiveError::prometheus("bad answer", format!("sample value {value:?}"))
                })?;
                samples.push(Sample { ts_ms, value });
            }
            if !samples.is_empty() {
                samples.sort_by_key(|s| s.ts_ms);
                out.push(Series {
                    name,
                    labels: item.metric,
                    samples,
                });
            }
        }
        Ok(out)
    }

    /// One instant query; `at` is an RFC 3339 time, or now
    async fn query(&self, query: &str, at: Option<&str>) -> ArchiveResult<Vec<Item>> {
        let shown = putio_sync_core::redact::url(&self.base);
        let url = format!("{}/api/v1/query", self.base);
        let mut request = self.http.get(&url).query(&[("query", query)]);
        if let Some(at) = at {
            request = request.query(&[("time", at)]);
        }
        let response = request.send().await.map_err(|e| {
            ArchiveError::prometheus(&format!("cannot reach Prometheus ({shown})"), describe(e))
        })?;
        let status = response.status();
        let body = response.text().await.map_err(|e| {
            ArchiveError::prometheus(
                &format!("cannot read the answer of Prometheus ({shown})"),
                describe(e),
            )
        })?;
        let reply: Option<Reply> = serde_json::from_str(&body).ok();
        if !status.is_success() || reply.as_ref().is_none_or(|r| r.status != "success") {
            let cause = reply
                .and_then(|r| r.error)
                .unwrap_or_else(|| "no error text".to_string());
            return Err(ArchiveError::prometheus(
                &format!("Prometheus ({shown}) answered HTTP {}", status.as_u16()),
                cause,
            ));
        }
        Ok(reply
            .and_then(|r| r.data)
            .map_or_else(Vec::new, |d| d.result))
    }
}

/// A request error with its causes, and without the URL (it may hold credentials)
fn describe(error: reqwest::Error) -> String {
    let error = error.without_url();
    let mut text = error.to_string();
    let mut source = std::error::Error::source(&error);
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::{Matcher, Server};

    fn day(text: &str) -> NaiveDate {
        text.parse().unwrap()
    }

    // 2026-11-01T00:00:00Z
    const START: i64 = 1_793_491_200;

    fn matrix(values: &str) -> String {
        format!(
            r#"{{"status":"success","data":{{"resultType":"matrix","result":[
            {{"metric":{{"__name__":"putio_sync:up:min5m","app":"putio-sync","instance":"nas:9742"}},
              "values":{values}}}]}}}}"#
        )
    }

    #[tokio::test]
    async fn fetch_day_asks_for_the_range_selector_and_keeps_the_day() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/api/v1/query")
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded(
                    "query".into(),
                    r#"{__name__=~"putio_sync:.+:[a-z]*5m",app="putio-sync",instance="nas:9742"}[1d]"#
                        .into(),
                ),
                Matcher::UrlEncoded("time".into(), "2026-11-02T00:00:00Z".into()),
            ]))
            .with_body(matrix(&format!(
                r#"[[{},"1"],[{},"0"],[{},"1"],[{},"1"]]"#,
                START - 300,
                START,
                START + 86_100,
                START + 86_400
            )))
            .create_async()
            .await;

        let client = PromClient::new(&server.url()).unwrap();
        let series = client
            .fetch_day("nas:9742", day("2026-11-01"))
            .await
            .unwrap();

        mock.assert_async().await;
        assert_eq!(series.len(), 1);
        assert_eq!(series[0].name, "putio_sync:up:min5m");
        assert_eq!(series[0].labels["instance"], "nas:9742");
        assert!(!series[0].labels.contains_key("__name__"));
        assert_eq!(
            series[0].samples,
            vec![
                Sample {
                    ts_ms: START * 1000,
                    value: 0.0
                },
                Sample {
                    ts_ms: (START + 86_100) * 1000,
                    value: 1.0
                },
            ]
        );
    }

    #[tokio::test]
    async fn fetch_day_with_no_samples_in_the_day_is_empty() {
        let mut server = Server::new_async().await;
        server
            .mock("GET", "/api/v1/query")
            .match_query(Matcher::Any)
            .with_body(matrix(&format!(r#"[[{},"1"]]"#, START - 300)))
            .create_async()
            .await;
        let client = PromClient::new(&server.url()).unwrap();
        assert!(client
            .fetch_day("nas:9742", day("2026-11-01"))
            .await
            .unwrap()
            .is_empty());

        server.reset();
        server
            .mock("GET", "/api/v1/query")
            .match_query(Matcher::Any)
            .with_body(r#"{"status":"success","data":{"resultType":"matrix","result":[]}}"#)
            .create_async()
            .await;
        assert!(client
            .fetch_day("nas:9742", day("2026-11-01"))
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn http_errors_and_error_bodies_are_prometheus_errors() {
        let mut server = Server::new_async().await;
        server
            .mock("GET", "/api/v1/query")
            .match_query(Matcher::Any)
            .with_status(500)
            .with_body("boom")
            .create_async()
            .await;
        let client = PromClient::new(&server.url()).unwrap();
        let err = client.fetch_day("i", day("2026-11-01")).await.unwrap_err();
        assert_eq!(err.exit_code(), 2);
        assert!(err.to_string().contains("500"), "{err}");

        server.reset();
        server
            .mock("GET", "/api/v1/query")
            .match_query(Matcher::Any)
            .with_body(r#"{"status":"error","errorType":"bad_data","error":"nope"}"#)
            .create_async()
            .await;
        let err = client.fetch_day("i", day("2026-11-01")).await.unwrap_err();
        assert_eq!(err.exit_code(), 2);
        assert!(err.to_string().contains("nope"), "{err}");
    }

    #[tokio::test]
    async fn an_unreachable_server_is_a_prometheus_error_without_credentials() {
        let client = PromClient::new("http://user:hunter2secret@127.0.0.1:1").unwrap();
        let err = client.fetch_day("i", day("2026-11-01")).await.unwrap_err();
        assert_eq!(err.exit_code(), 2);
        assert!(!err.to_string().contains("hunter2secret"), "{err}");
    }

    #[test]
    fn a_bad_url_is_an_argument_error() {
        assert_eq!(PromClient::new("not a url").err().unwrap().exit_code(), 1);
    }

    fn instances(names: &[&str]) -> String {
        let rows: Vec<String> = names
            .iter()
            .map(|n| format!(r#"{{"metric":{{"instance":"{n}"}},"value":[1793491200,"1"]}}"#))
            .collect();
        format!(
            r#"{{"status":"success","data":{{"resultType":"vector","result":[{}]}}}}"#,
            rows.join(",")
        )
    }

    #[tokio::test]
    async fn instance_is_detected_when_there_is_only_one() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/api/v1/query")
            .match_query(Matcher::UrlEncoded(
                "query".into(),
                r#"count by (instance) (up{app="putio-sync"})"#.into(),
            ))
            .with_body(instances(&["nas:9742"]))
            .create_async()
            .await;
        let client = PromClient::new(&server.url()).unwrap();
        assert_eq!(client.instance(None).await.unwrap(), "nas:9742");
        mock.assert_async().await;
        // A given instance needs no query
        assert_eq!(client.instance(Some("other")).await.unwrap(), "other");
    }

    #[tokio::test]
    async fn several_instances_need_an_explicit_choice() {
        let mut server = Server::new_async().await;
        server
            .mock("GET", "/api/v1/query")
            .match_query(Matcher::Any)
            .with_body(instances(&["a:9742", "b:9742"]))
            .create_async()
            .await;
        let client = PromClient::new(&server.url()).unwrap();
        let err = client.instance(None).await.unwrap_err();
        assert_eq!(err.exit_code(), 1);
        assert!(err.to_string().contains("--instance"), "{err}");
        assert!(err.to_string().contains("a:9742"), "{err}");
    }

    #[tokio::test]
    async fn no_instance_is_a_prometheus_error() {
        let mut server = Server::new_async().await;
        server
            .mock("GET", "/api/v1/query")
            .match_query(Matcher::Any)
            .with_body(instances(&[]))
            .create_async()
            .await;
        let client = PromClient::new(&server.url()).unwrap();
        assert_eq!(client.instance(None).await.unwrap_err().exit_code(), 2);
    }
}
