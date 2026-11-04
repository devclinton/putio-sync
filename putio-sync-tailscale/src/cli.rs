//! The `tailscale` CLI, for when the socket isn't there (the macOS app keeps its own).

use std::path::{Path, PathBuf};
use std::time::Duration;

/// Where the macOS app keeps its CLI
const MACOS_APP: &str = "/Applications/Tailscale.app/Contents/MacOS/Tailscale";

/// The `tailscale` binary: `PATH` first, then the macOS app
pub fn find() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join("tailscale"))
        .find(|p| is_executable(p))
        .or_else(|| {
            let app = PathBuf::from(MACOS_APP);
            is_executable(&app).then_some(app)
        })
}

fn is_executable(p: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        p.metadata()
            .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        p.is_file()
    }
}

/// Run the CLI with `args` and return its stdout. A non-zero exit is an error with the
/// first line of stderr.
pub async fn run(bin: &Path, args: &[&str], timeout: Duration) -> Result<Vec<u8>, String> {
    let child = tokio::process::Command::new(bin)
        .args(args)
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true)
        .output();
    let out = tokio::time::timeout(timeout, child)
        .await
        .map_err(|_| "the tailscale CLI didn't answer in time".to_string())?
        .map_err(|e| format!("can't run {}: {e}", bin.display()))?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        let line = err
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("")
            .trim();
        Err(if line.is_empty() {
            format!("tailscale {} failed", args.join(" "))
        } else {
            line.to_string()
        })
    }
}
