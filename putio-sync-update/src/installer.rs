//! "Update now" for the macOS `.pkg` and Windows `-setup.exe` installs (research R9): download
//! the new installer, check it against the signed `SHA256SUMS`, and open it when the daemon
//! runs in the user's desktop session. The installer owns its files and its service setup,
//! so it takes over from there.

use crate::channel::Channel;
use crate::fetch::{self, PrepareError, Progress};
use crate::github::ReleaseInfo;
use crate::version::Version;
use std::path::{Path, PathBuf};

/// The installer asset for `channel` on this machine
pub fn asset_name(channel: &Channel, to: Version) -> Option<String> {
    asset_name_for(channel, to, std::env::consts::ARCH)
}

fn asset_name_for(channel: &Channel, to: Version, arch: &str) -> Option<String> {
    match (channel, arch) {
        (Channel::MacPkg, "aarch64") => Some(format!("putio-sync-{to}-macos-arm64.pkg")),
        (Channel::WindowsSetup, "x86_64") => Some(format!("putio-sync-{to}-windows-x64-setup.exe")),
        (Channel::WindowsSetup, "aarch64") => {
            Some(format!("putio-sync-{to}-windows-arm64-setup.exe"))
        }
        _ => None,
    }
}

/// What happened to the downloaded installer
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// It is open on the user's desktop
    Opened(PathBuf),
    /// No desktop session (the setup.exe service runs as SYSTEM): the user opens it
    Downloaded(PathBuf),
}

/// Download `name` from `release` into `dir` (the config folder's `updates/`), checked
/// against the signed list
pub async fn download(
    client: &reqwest::Client,
    release: &ReleaseInfo,
    name: &str,
    dir: &Path,
    pubkey: &str,
    progress: &Progress,
) -> Result<PathBuf, PrepareError> {
    let sums = fetch::signed_sums(client, release, pubkey).await?;
    let asset = release
        .asset(name)
        .ok_or_else(|| PrepareError::Missing(name.to_string()))?;
    std::fs::create_dir_all(dir).map_err(|e| PrepareError::Io(e.to_string()))?;
    let file = fetch::download_to(client, asset, dir, &sums, progress).await?;
    let path = dir.join(name);
    file.persist(&path)
        .map_err(|e| PrepareError::Io(e.error.to_string()))?;
    Ok(path)
}

/// Open `path` when this process runs in the user's desktop session
pub fn open_or_report(path: PathBuf) -> Outcome {
    if !in_desktop_session() {
        return Outcome::Downloaded(path);
    }
    match open(&path) {
        Ok(()) => Outcome::Opened(path),
        Err(e) => {
            tracing::warn!(error = %e, path = %path.display(), "Could not open the installer");
            Outcome::Downloaded(path)
        }
    }
}

/// Started by the user's launchd agent, not as root
#[cfg(target_os = "macos")]
pub fn in_desktop_session() -> bool {
    std::env::var("XPC_SERVICE_NAME").as_deref() == Ok("com.devclinton.putio-sync")
        && unsafe { libc::geteuid() } != 0
}

/// The active console session is ours (the SYSTEM service runs in session 0)
#[cfg(windows)]
pub fn in_desktop_session() -> bool {
    use windows_sys::Win32::System::RemoteDesktop::{
        ProcessIdToSessionId, WTSGetActiveConsoleSessionId,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcessId;
    let mut ours = 0u32;
    // SAFETY: plain calls with a valid out pointer
    unsafe {
        ProcessIdToSessionId(GetCurrentProcessId(), &mut ours) != 0
            && ours != 0
            && WTSGetActiveConsoleSessionId() == ours
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
pub fn in_desktop_session() -> bool {
    false
}

#[cfg(target_os = "macos")]
fn open(path: &Path) -> std::io::Result<()> {
    std::process::Command::new("open")
        .arg(path)
        .spawn()
        .map(|_| ())
}

/// `ShellExecuteW` with `runas`, so Windows asks for elevation
#[cfg(windows)]
fn open(path: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let wide = |s: &std::ffi::OsStr| s.encode_wide().chain([0]).collect::<Vec<u16>>();
    let verb = wide("runas".as_ref());
    let file = wide(path.as_os_str());
    // SAFETY: null-terminated UTF-16 strings that outlive the call
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    // Values above 32 mean success
    if result as isize > 32 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
fn open(_path: &Path) -> std::io::Result<()> {
    Err(std::io::Error::other("no desktop session"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::Asset;
    use crate::verify::tests::{line, sign, TEST_PUBKEY};

    fn v(s: &str) -> Version {
        s.parse().unwrap()
    }

    #[test]
    fn asset_names() {
        let to = v("0.4.0");
        assert_eq!(
            asset_name_for(&Channel::MacPkg, to, "aarch64").as_deref(),
            Some("putio-sync-0.4.0-macos-arm64.pkg")
        );
        assert_eq!(
            asset_name_for(&Channel::WindowsSetup, to, "x86_64").as_deref(),
            Some("putio-sync-0.4.0-windows-x64-setup.exe")
        );
        assert_eq!(
            asset_name_for(&Channel::WindowsSetup, to, "aarch64").as_deref(),
            Some("putio-sync-0.4.0-windows-arm64-setup.exe")
        );
        assert_eq!(asset_name_for(&Channel::MacPkg, to, "x86_64"), None);
        assert_eq!(asset_name_for(&Channel::GithubArchive, to, "x86_64"), None);
    }

    async fn release(server: &mut mockito::ServerGuard, pkg: &[u8], listed: &str) -> ReleaseInfo {
        let name = "putio-sync-0.4.0-macos-arm64.pkg";
        let files = [
            (name, pkg.to_vec()),
            (fetch::SUMS, listed.as_bytes().to_vec()),
            (fetch::SIGNATURE, sign(listed.as_bytes()).into_bytes()),
        ];
        let mut assets = Vec::new();
        for (name, body) in files {
            let path = format!("/d/{name}");
            server
                .mock("GET", path.as_str())
                .with_body(&body)
                .create_async()
                .await;
            assets.push(Asset {
                name: name.into(),
                url: format!("{}{path}", server.url()),
                size: body.len() as u64,
            });
        }
        ReleaseInfo {
            version: v("0.4.0"),
            published_at: None,
            html_url: String::new(),
            body: String::new(),
            assets,
        }
    }

    #[tokio::test]
    async fn downloads_and_checks_the_installer() {
        let name = "putio-sync-0.4.0-macos-arm64.pkg";
        let mut server = mockito::Server::new_async().await;
        let rel = release(&mut server, b"pkg bytes", &line(b"pkg bytes", name)).await;
        let dir = tempfile::tempdir().unwrap();
        let updates = dir.path().join("updates");
        let path = download(
            &crate::github::client(),
            &rel,
            name,
            &updates,
            TEST_PUBKEY,
            &Progress::default(),
        )
        .await
        .unwrap();
        assert_eq!(path, updates.join(name));
        assert_eq!(std::fs::read(&path).unwrap(), b"pkg bytes");
    }

    #[tokio::test]
    async fn refuses_an_installer_that_does_not_match() {
        let name = "putio-sync-0.4.0-macos-arm64.pkg";
        let mut server = mockito::Server::new_async().await;
        let rel = release(&mut server, b"tampered", &line(b"pkg bytes", name)).await;
        let dir = tempfile::tempdir().unwrap();
        let err = download(
            &crate::github::client(),
            &rel,
            name,
            dir.path(),
            TEST_PUBKEY,
            &Progress::default(),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, PrepareError::Verify(_)), "{err:?}");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[cfg(not(any(target_os = "macos", windows)))]
    #[test]
    fn elsewhere_it_only_reports_the_path() {
        let path = PathBuf::from("/tmp/x.pkg");
        assert_eq!(open_or_report(path.clone()), Outcome::Downloaded(path));
    }
}
