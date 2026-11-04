//! Connectivity checks, all read from the one account probe

use super::Ctx;
use crate::report::{Finding, Status};
use putio_sync_client::ClientError;
use std::time::Duration;

/// Answers slower than this give a warning
const SLOW: Duration = Duration::from_secs(2);
/// Less of the rate limit left than this, in percent, gives a warning
const RATE_LIMIT_WARNING: u64 = 10;
/// Clock differences that give a warning / a problem, in seconds
const SKEW_WARNING: i64 = 60;
const SKEW_PROBLEM: i64 = 300;

pub(crate) async fn reachability(ctx: &Ctx) -> Finding {
    let (probe, _) = ctx.probe().await;
    let ms = probe.elapsed.as_millis();
    match (&probe.result, probe.http_status) {
        (Err(ClientError::AuthError(_)), None) => {
            Finding::skipped("No Put.io token is configured, so nothing was sent.")
        }
        (Err(e), None) => {
            Finding::new(Status::Problem, format!("Put.io could not be reached: {e}"))
                .suggest("Check the NAS's internet connection and DNS, then run the doctor again.")
        }
        (_, Some(status)) if status.is_server_error() => Finding::new(
            Status::Warning,
            format!("Put.io answered with an error ({}).", status.as_u16()),
        )
        .suggest("Put.io may be having trouble; try again later."),
        _ if probe.elapsed > SLOW => Finding::new(
            Status::Warning,
            format!("Put.io answered, but slowly ({ms} ms)."),
        )
        .suggest("Downloads and checks may be slow; if it keeps happening, check the network."),
        _ => Finding::ok(format!("Put.io answered in {ms} ms.")),
    }
}

pub(crate) async fn rate_limit(ctx: &Ctx) -> Finding {
    let (probe, _) = ctx.probe().await;
    if probe.rate_limited() {
        return Finding::new(Status::Warning, "Put.io is limiting requests right now.")
            .suggest("putio-sync slows down on its own; wait a few minutes.");
    }
    match probe.rate_limit {
        Some((limit, remaining)) if limit > 0 => {
            let summary = format!("{remaining} of {limit} requests left.");
            if remaining * 100 < limit * RATE_LIMIT_WARNING {
                Finding::new(Status::Warning, summary).suggest(
                    "Something is using the Put.io API heavily; check other apps using this token.",
                )
            } else {
                Finding::ok(summary)
            }
        }
        _ if probe.http_status.is_some() => Finding::skipped("Put.io did not report a rate limit."),
        _ => Finding::skipped("Put.io could not be reached."),
    }
}

pub(crate) async fn clock_skew(ctx: &Ctx) -> Finding {
    let (probe, answered) = ctx.probe().await;
    let Some(server) = probe.server_date else {
        return Finding::skipped("Put.io's clock could not be read.");
    };
    let skew = (*answered - server).num_seconds();
    let direction = if skew > 0 { "ahead of" } else { "behind" };
    let summary = format!(
        "This machine's clock is {}s {direction} Put.io's.",
        skew.abs()
    );
    let status = match skew.abs() {
        s if s > SKEW_PROBLEM => Status::Problem,
        s if s > SKEW_WARNING => Status::Warning,
        _ => return Finding::ok("This machine's clock matches Put.io's."),
    };
    Finding::new(status, summary)
        .suggest("Turn on time sync (NTP) on the NAS; ages and schedules depend on the clock.")
}

/// Tailscale (feature 036): only graded when sign-in or tailnet-only is on
pub(crate) async fn tailscale(ctx: &Ctx) -> Finding {
    use putio_sync_tailscale::{tailnet_bind, Cli, State, TailscaleService};
    let cfg = &ctx.config.tailscale;
    if !cfg.identity_sign_in && !cfg.tailnet_only {
        return Finding::ok("Tailscale sign-in and listening on the tailnet are off.");
    }
    if cfg.identity_sign_in && cfg.allowed_users.is_empty() {
        return Finding::new(
            Status::Warning,
            "Tailscale sign-in is on, but no Tailscale user is allowed, so nobody gets in that way.",
        )
        .suggest("Add Tailscale logins under Allowed users in Settings, Tailscale.");
    }
    let built;
    let ts = match &ctx.tailscale {
        Some(ts) => ts.as_ref(),
        None => {
            built = TailscaleService::new(cfg.socket_path(), Cli::Auto);
            &built
        }
    };
    let status = ts.status().await;
    let feature = if cfg.tailnet_only {
        "Listening on the tailnet only"
    } else {
        "Tailscale sign-in"
    };
    if status.state != State::Running {
        let suggestion = match status.state {
            State::NotFound => format!(
                "Install Tailscale, or mount tailscaled's socket ({}) into the container.",
                cfg.socket_path().display()
            ),
            State::NeedsLogin => "Sign this machine in to Tailscale (tailscale up).".to_string(),
            State::Stopped => "Start Tailscale (tailscale up).".to_string(),
            _ => "Check that tailscaled is running and its socket can be read.".to_string(),
        };
        let mut summary = format!(
            "{feature} is on, but Tailscale is {}.",
            status.state.as_str()
        );
        if let Some(e) = &status.error {
            summary = format!("{summary} {e}.");
        }
        return Finding::new(Status::Warning, summary).suggest(suggestion);
    }
    if cfg.tailnet_only {
        let port = ctx
            .config
            .resolve_api()
            .map(|r| r.value.bind.port())
            .unwrap_or(15432);
        if let Err(reason) = tailnet_bind(&status, port) {
            return Finding::new(
                Status::Warning,
                format!("Listening on the tailnet only is on, but there is no tailnet address: {reason}."),
            )
            .suggest("Turn it off in Settings, Tailscale, or give Tailscale a tailnet address on this machine.");
        }
    }
    let name = status
        .dns_name
        .clone()
        .or_else(|| status.ipv4().map(|ip| ip.to_string()))
        .unwrap_or_else(|| "this machine".to_string());
    Finding::ok(format!("Tailscale is running as {name}."))
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;
    use chrono::{DateTime, Utc};
    use putio_sync_client::AccountProbe;
    use reqwest::StatusCode;

    fn with_probe(probe: AccountProbe, answered: DateTime<Utc>) -> (tempfile::TempDir, Ctx) {
        let dir = tempfile::tempdir().unwrap();
        let c = ctx("http://127.0.0.1:9", dir.path(), None);
        c.probe.set((probe, answered)).unwrap();
        (dir, c)
    }

    fn answered(status: u16, elapsed_ms: u64) -> AccountProbe {
        AccountProbe {
            result: Err(ClientError::ApiError("x".into())),
            http_status: Some(StatusCode::from_u16(status).unwrap()),
            elapsed: Duration::from_millis(elapsed_ms),
            server_date: None,
            rate_limit: None,
        }
    }

    #[tokio::test]
    async fn reachability_grades() {
        let (_d, c) = with_probe(answered(200, 120), Utc::now());
        assert_eq!(reachability(&c).await.status, Status::Ok);
        let (_d, c) = with_probe(answered(200, 2500), Utc::now());
        assert_eq!(reachability(&c).await.status, Status::Warning);
        let (_d, c) = with_probe(answered(503, 10), Utc::now());
        assert_eq!(reachability(&c).await.status, Status::Warning);

        let mut none = answered(200, 1);
        none.http_status = None;
        none.result = Err(ClientError::InternalError("connection refused".into()));
        let (_d, c) = with_probe(none, Utc::now());
        assert_eq!(reachability(&c).await.status, Status::Problem);

        let mut no_token = answered(200, 1);
        no_token.http_status = None;
        no_token.result = Err(ClientError::AuthError("no token".into()));
        let (_d, c) = with_probe(no_token, Utc::now());
        assert_eq!(reachability(&c).await.status, Status::Skipped);
    }

    #[tokio::test]
    async fn rate_limit_grades() {
        let mut p = answered(200, 1);
        p.rate_limit = Some((1000, 900));
        let (_d, c) = with_probe(p, Utc::now());
        assert_eq!(rate_limit(&c).await.status, Status::Ok);

        let mut p = answered(200, 1);
        p.rate_limit = Some((1000, 50));
        let (_d, c) = with_probe(p, Utc::now());
        assert_eq!(rate_limit(&c).await.status, Status::Warning);

        let (_d, c) = with_probe(answered(429, 1), Utc::now());
        assert_eq!(rate_limit(&c).await.status, Status::Warning);

        let (_d, c) = with_probe(answered(200, 1), Utc::now());
        assert_eq!(rate_limit(&c).await.status, Status::Skipped);
    }

    #[tokio::test]
    async fn clock_skew_grades() {
        let now = Utc::now();
        for (offset, status) in [
            (5, Status::Ok),
            (-90, Status::Warning),
            (400, Status::Problem),
        ] {
            let mut p = answered(200, 1);
            p.server_date = Some(now - chrono::Duration::seconds(offset));
            let (_d, c) = with_probe(p, now);
            let f = clock_skew(&c).await;
            assert_eq!(f.status, status, "{offset}: {}", f.summary);
        }
        let (_d, c) = with_probe(answered(200, 1), now);
        assert_eq!(clock_skew(&c).await.status, Status::Skipped);
    }

    /// A temp dir under /tmp: unix socket paths must stay under about 104 bytes, which a
    /// long `TMPDIR` breaks
    fn short_dir() -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix("ts")
            .tempdir_in("/tmp")
            .unwrap()
    }

    /// A fake tailscaled answering every request with `status`
    fn fake_tailscaled(dir: &std::path::Path, status: &'static str) -> std::path::PathBuf {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let socket = dir.join("ts.sock");
        let listener = tokio::net::UnixListener::bind(&socket).unwrap();
        tokio::spawn(async move {
            while let Ok((mut conn, _)) = listener.accept().await {
                let mut buf = [0u8; 2048];
                let _ = conn.read(&mut buf).await;
                let req = String::from_utf8_lossy(&buf);
                let body = if req.starts_with("GET /localapi/v0/status") {
                    status
                } else {
                    "{}"
                };
                let resp = format!("HTTP/1.0 200 OK\r\n\r\n{body}");
                let _ = conn.write_all(resp.as_bytes()).await;
            }
        });
        socket
    }

    fn with_tailscale(
        socket: std::path::PathBuf,
        edit: impl FnOnce(&mut putio_sync_core::config::TailscaleConfig),
    ) -> (tempfile::TempDir, Ctx) {
        let dir = tempfile::tempdir().unwrap();
        let mut c = ctx("http://127.0.0.1:9", dir.path(), None);
        edit(&mut c.config.tailscale);
        c.tailscale = Some(std::sync::Arc::new(
            putio_sync_tailscale::TailscaleService::new(socket, putio_sync_tailscale::Cli::Off)
                .with_timeout(Duration::from_millis(300)),
        ));
        (dir, c)
    }

    #[tokio::test]
    async fn tailscale_grades() {
        let dir = short_dir();
        let missing = dir.path().join("none.sock");

        // Nothing on: ok without asking
        let (_d, c) = with_tailscale(missing.clone(), |_| {});
        assert_eq!(tailscale(&c).await.status, Status::Ok);

        // Sign-in on with nobody allowed
        let (_d, c) = with_tailscale(missing.clone(), |t| t.identity_sign_in = true);
        let f = tailscale(&c).await;
        assert_eq!(f.status, Status::Warning);
        assert!(f.summary.contains("no Tailscale user"), "{}", f.summary);

        // Sign-in on, Tailscale missing
        let (_d, c) = with_tailscale(missing.clone(), |t| {
            t.identity_sign_in = true;
            t.allowed_users = vec!["alice@example.com".into()];
        });
        let f = tailscale(&c).await;
        assert_eq!(f.status, Status::Warning);
        assert!(f.summary.contains("not found"), "{}", f.summary);

        // Tailnet only with a running node
        let running = r#"{"BackendState":"Running","TUN":true,"TailscaleIPs":["100.101.102.103"],"Self":{"DNSName":"nas.ts.net."},"CurrentTailnet":{"MagicDNSEnabled":true}}"#;
        let socket = fake_tailscaled(dir.path(), running);
        let (_d, c) = with_tailscale(socket, |t| t.tailnet_only = true);
        let f = tailscale(&c).await;
        assert_eq!(f.status, Status::Ok, "{}", f.summary);
        assert!(f.summary.contains("nas.ts.net"));

        // Tailnet only in userspace mode: no address to listen on
        let dir2 = short_dir();
        let userspace =
            r#"{"BackendState":"Running","TUN":false,"TailscaleIPs":["100.101.102.103"]}"#;
        let socket = fake_tailscaled(dir2.path(), userspace);
        let (_d, c) = with_tailscale(socket, |t| t.tailnet_only = true);
        let f = tailscale(&c).await;
        assert_eq!(f.status, Status::Warning);
        assert!(f.summary.contains("userspace"), "{}", f.summary);
    }
}
