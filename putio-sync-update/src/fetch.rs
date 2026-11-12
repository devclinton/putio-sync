//! Release asset names for the running platform, and the download that turns one into a
//! verified binary (contracts/release-assets.md).

use crate::github::{Asset, ReleaseInfo};
use crate::verify::{self, Sums, VerifyError};
use crate::version::Version;
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// One of the five platforms that get archive releases
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Target(&'static str);

impl Target {
    /// The platform this binary was built for, or `None` where no archive is published
    /// (Intel macOS included)
    pub fn current() -> Option<Self> {
        Self::for_platform(std::env::consts::OS, std::env::consts::ARCH)
    }

    fn for_platform(os: &str, arch: &str) -> Option<Self> {
        let name = match (os, arch) {
            ("linux", "x86_64") => "linux-x86_64",
            ("linux", "aarch64") => "linux-aarch64",
            ("macos", "aarch64") => "macos-arm64",
            ("windows", "x86_64") => "windows-x64",
            ("windows", "aarch64") => "windows-arm64",
            _ => return None,
        };
        Some(Self(name))
    }

    /// The target as asset names spell it
    pub fn as_str(&self) -> &'static str {
        self.0
    }

    /// The xz-compressed bare binary
    pub fn full(&self, version: Version) -> String {
        format!("putio-sync-{version}-{}.xz", self.0)
    }

    /// The bsdiff patch from `from` to `to`, xz-compressed
    pub fn patch(&self, from: Version, to: Version) -> String {
        format!("putio-sync-{from}-to-{to}-{}.bsdiff.xz", self.0)
    }

    /// The `SHA256SUMS` line for the uncompressed binary (no file has this name)
    pub fn bin(&self, version: Version) -> String {
        format!("putio-sync-{version}-{}.bin", self.0)
    }
}

/// The signed list of hashes
pub const SUMS: &str = "SHA256SUMS";
/// Its minisign signature
pub const SIGNATURE: &str = "SHA256SUMS.minisig";

/// What a swap downloads
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// A bsdiff patch from the running version
    Patch,
    /// The whole binary
    Full,
}

impl Kind {
    /// The `kind` label of `putio_sync_update_attempts_total`
    pub fn label(&self) -> &'static str {
        match self {
            Self::Patch => "patch",
            Self::Full => "full",
        }
    }
}

/// The asset an update from `running` downloads: the patch when the release lists one
/// (FR-014), else the full binary
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub kind: Kind,
    pub asset: Asset,
}

/// The plan for `release`, or `None` when it has neither asset for `target`
pub fn plan(release: &ReleaseInfo, running: Version, target: Target) -> Option<Plan> {
    if let Some(asset) = release.asset(&target.patch(running, release.version)) {
        return Some(Plan {
            kind: Kind::Patch,
            asset: asset.clone(),
        });
    }
    release
        .asset(&target.full(release.version))
        .map(|asset| Plan {
            kind: Kind::Full,
            asset: asset.clone(),
        })
}

/// How far a download has got, for `GET /update`
#[derive(Debug, Default)]
pub struct Progress {
    /// Bytes received so far
    pub bytes: AtomicU64,
    /// The download is complete and being checked
    pub verifying: AtomicBool,
}

impl Progress {
    /// Back to nothing downloaded
    pub fn reset(&self) {
        self.bytes.store(0, Ordering::Relaxed);
        self.verifying.store(false, Ordering::Relaxed);
    }
}

/// A binary that matched its release's signed `.bin` line, ready for the swap
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedBinary {
    pub version: Version,
    pub kind: Kind,
    pub bytes: Vec<u8>,
}

/// Why an update could not be prepared. Nothing is left on disk after any of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrepareError {
    /// A signature or hash check refused it
    Verify(VerifyError),
    /// The release has no asset this update needs
    Missing(String),
    /// No answer, or one other than `200`
    Download(String),
    /// The temp file could not be written (a full disk, a read-only folder)
    Io(String),
    /// The xz or the patch would not decode
    Decode(String),
}

impl std::fmt::Display for PrepareError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Verify(e) => write!(f, "{e}"),
            Self::Missing(name) => write!(f, "the release has no {name}"),
            Self::Download(e) => write!(f, "download failed: {e}"),
            Self::Io(e) => write!(f, "could not write the download: {e}"),
            Self::Decode(e) => write!(f, "could not decode the download: {e}"),
        }
    }
}

impl std::error::Error for PrepareError {}

impl From<VerifyError> for PrepareError {
    fn from(e: VerifyError) -> Self {
        e.record();
        Self::Verify(e)
    }
}

/// The release's `SHA256SUMS`, checked against its signature and `pubkey`
pub async fn signed_sums(
    client: &reqwest::Client,
    release: &ReleaseInfo,
    pubkey: &str,
) -> Result<Sums, PrepareError> {
    let sums = small(client, release, SUMS).await?;
    let signature = match release.asset(SIGNATURE) {
        Some(_) => small(client, release, SIGNATURE).await?,
        None => Vec::new(),
    };
    let signature = String::from_utf8_lossy(&signature);
    Ok(verify::verify_list(&sums, &signature, pubkey)?)
}

/// Download, check and decode the update to `release` for `target` (T031):
///
/// 1. `SHA256SUMS` and its signature, checked against `pubkey`
/// 2. the patch from `running` if listed, else the full binary, streamed to a temp file
///    in `dir` (the binary's folder) while `progress` counts the bytes, then checked
///    against its line
/// 3. decompressed, patched onto `current_exe` for a patch, and checked against the
///    `.bin` line
#[allow(clippy::too_many_arguments)]
pub async fn prepare(
    client: &reqwest::Client,
    release: &ReleaseInfo,
    running: Version,
    current_exe: &[u8],
    target: Target,
    dir: &Path,
    pubkey: &str,
    progress: &Progress,
) -> Result<PreparedBinary, PrepareError> {
    let sums = signed_sums(client, release, pubkey).await?;
    let plan = plan(release, running, target)
        .ok_or_else(|| PrepareError::Missing(target.full(release.version)))?;
    let file = download_to(client, &plan.asset, dir, &sums, progress).await?;

    let compressed = std::fs::read(file.path()).map_err(|e| PrepareError::Io(e.to_string()))?;
    drop(file);
    let mut decoded = Vec::new();
    lzma_rs::xz_decompress(&mut compressed.as_slice(), &mut decoded)
        .map_err(|e| PrepareError::Decode(format!("{}: {e}", plan.asset.name)))?;
    let bytes = match plan.kind {
        Kind::Full => decoded,
        Kind::Patch => {
            let mut out = Vec::new();
            bsdiff::patch(current_exe, &mut decoded.as_slice(), &mut out)
                .map_err(|e| PrepareError::Decode(format!("{}: {e}", plan.asset.name)))?;
            out
        }
    };
    sums.check(&bytes, &target.bin(release.version))
        .map_err(VerifyError::Binary)?;
    Ok(PreparedBinary {
        version: release.version,
        kind: plan.kind,
        bytes,
    })
}

/// A small asset, read into memory
pub(crate) async fn small(
    client: &reqwest::Client,
    release: &ReleaseInfo,
    name: &str,
) -> Result<Vec<u8>, PrepareError> {
    let asset = release
        .asset(name)
        .ok_or_else(|| PrepareError::Missing(name.to_string()))?;
    let response = get(client, asset).await?;
    let bytes = response
        .bytes()
        .await
        .map_err(|e| PrepareError::Download(e.without_url().to_string()))?;
    Ok(bytes.to_vec())
}

async fn get(client: &reqwest::Client, asset: &Asset) -> Result<reqwest::Response, PrepareError> {
    let response = client
        .get(&asset.url)
        .timeout(DOWNLOAD_TIMEOUT)
        .send()
        .await
        .map_err(|e| PrepareError::Download(e.without_url().to_string()))?;
    if !response.status().is_success() {
        return Err(PrepareError::Download(format!(
            "{} answered {}",
            asset.name,
            response.status().as_u16()
        )));
    }
    Ok(response)
}

/// The longest one asset may take (the full binaries are about 13 MB)
const DOWNLOAD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15 * 60);

/// Stream `asset` to a temp file in `dir` and check it against `sums`. The file is deleted
/// when the returned handle drops, and on every error.
pub(crate) async fn download_to(
    client: &reqwest::Client,
    asset: &Asset,
    dir: &Path,
    sums: &Sums,
    progress: &Progress,
) -> Result<tempfile::NamedTempFile, PrepareError> {
    if !sums.contains(&asset.name) {
        return Err(VerifyError::Download(format!("{} is not in SHA256SUMS", asset.name)).into());
    }
    let mut response = get(client, asset).await?;
    let mut file = tempfile::Builder::new()
        .prefix(".putio-sync-update-")
        .tempfile_in(dir)
        .map_err(|e| PrepareError::Io(e.to_string()))?;
    let mut hasher = Sha256::new();
    progress.reset();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| PrepareError::Download(e.without_url().to_string()))?
    {
        hasher.update(&chunk);
        file.write_all(&chunk)
            .map_err(|e| PrepareError::Io(e.to_string()))?;
        progress
            .bytes
            .fetch_add(chunk.len() as u64, Ordering::Relaxed);
    }
    progress.verifying.store(true, Ordering::Relaxed);
    file.as_file()
        .sync_all()
        .map_err(|e| PrepareError::Io(e.to_string()))?;
    sums.check_hash(&hasher.finalize().into(), &asset.name)
        .map_err(VerifyError::Download)?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::verify::tests::{line, sign, TEST_PUBKEY};

    #[test]
    fn maps_the_five_archive_platforms() {
        let name = |os, arch| Target::for_platform(os, arch).map(|t| t.as_str());
        assert_eq!(name("linux", "x86_64"), Some("linux-x86_64"));
        assert_eq!(name("linux", "aarch64"), Some("linux-aarch64"));
        assert_eq!(name("macos", "aarch64"), Some("macos-arm64"));
        assert_eq!(name("windows", "x86_64"), Some("windows-x64"));
        assert_eq!(name("windows", "aarch64"), Some("windows-arm64"));
        assert_eq!(name("macos", "x86_64"), None);
        assert_eq!(name("freebsd", "x86_64"), None);
        assert_eq!(name("linux", "arm"), None);
    }

    #[test]
    fn asset_names() {
        let t = Target::for_platform("linux", "x86_64").unwrap();
        let (a, b) = ("0.2.0".parse().unwrap(), "0.3.0".parse().unwrap());
        assert_eq!(t.full(b), "putio-sync-0.3.0-linux-x86_64.xz");
        assert_eq!(
            t.patch(a, b),
            "putio-sync-0.2.0-to-0.3.0-linux-x86_64.bsdiff.xz"
        );
        assert_eq!(t.bin(b), "putio-sync-0.3.0-linux-x86_64.bin");
    }

    /// Two binaries a few bytes apart, a 0.3.0 → 0.4.0 patch and xz for each
    struct Fixture {
        old: Vec<u8>,
        new: Vec<u8>,
        full_xz: Vec<u8>,
        patch_xz: Vec<u8>,
    }

    fn xz(bytes: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        lzma_rs::xz_compress(&mut &bytes[..], &mut out).unwrap();
        out
    }

    fn fixture() -> Fixture {
        let old: Vec<u8> = (0..200_000u32).map(|i| (i * 7 % 251) as u8).collect();
        let mut new = old.clone();
        new[1000] ^= 0xff;
        new.extend_from_slice(b"version 0.4.0");
        let mut patch = Vec::new();
        bsdiff::diff(&old, &new, &mut patch).unwrap();
        Fixture {
            full_xz: xz(&new),
            patch_xz: xz(&patch),
            old,
            new,
        }
    }

    fn target() -> Target {
        Target::for_platform("linux", "x86_64").unwrap()
    }

    fn v(s: &str) -> Version {
        s.parse().unwrap()
    }

    /// A fake 0.4.0 release on `server` with `files` and a signed SHA256SUMS of `listed`
    async fn release(
        server: &mut mockito::ServerGuard,
        files: &[(&str, Vec<u8>)],
        listed: &str,
    ) -> ReleaseInfo {
        let signature = sign(listed.as_bytes());
        let mut assets = Vec::new();
        let all = files.iter().cloned().chain([
            (SUMS, listed.as_bytes().to_vec()),
            (SIGNATURE, signature.into_bytes()),
        ]);
        for (name, body) in all {
            let path = format!("/download/v0.4.0/{name}");
            server
                .mock("GET", path.as_str())
                .with_body(&body)
                .create_async()
                .await;
            assets.push(Asset {
                name: name.to_string(),
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

    fn sums_for(f: &Fixture) -> String {
        let t = target();
        line(&f.patch_xz, &t.patch(v("0.3.0"), v("0.4.0")))
            + &line(&f.full_xz, &t.full(v("0.4.0")))
            + &line(&f.new, &t.bin(v("0.4.0")))
    }

    async fn run(
        rel: &ReleaseInfo,
        f: &Fixture,
        dir: &Path,
    ) -> Result<PreparedBinary, PrepareError> {
        let progress = Progress::default();
        prepare(
            &crate::github::client(),
            rel,
            v("0.3.0"),
            &f.old,
            target(),
            dir,
            TEST_PUBKEY,
            &progress,
        )
        .await
    }

    fn left_in(dir: &Path) -> Vec<String> {
        std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect()
    }

    #[tokio::test]
    async fn takes_the_patch_when_listed() {
        let f = fixture();
        let t = target();
        let mut server = mockito::Server::new_async().await;
        let files = [
            (
                t.patch(v("0.3.0"), v("0.4.0")).leak() as &str,
                f.patch_xz.clone(),
            ),
            (t.full(v("0.4.0")).leak(), f.full_xz.clone()),
        ];
        let rel = release(&mut server, &files, &sums_for(&f)).await;
        assert_eq!(plan(&rel, v("0.3.0"), t).unwrap().kind, Kind::Patch);
        let dir = tempfile::tempdir().unwrap();
        let got = run(&rel, &f, dir.path()).await.unwrap();
        assert_eq!(got.kind, Kind::Patch);
        assert_eq!(got.bytes, f.new);
        assert!(left_in(dir.path()).is_empty());
    }

    #[tokio::test]
    async fn falls_back_to_the_full_binary() {
        let f = fixture();
        let t = target();
        let mut server = mockito::Server::new_async().await;
        let files = [(t.full(v("0.4.0")).leak() as &str, f.full_xz.clone())];
        let rel = release(&mut server, &files, &sums_for(&f)).await;
        // a version with no patch takes the full binary too
        assert_eq!(plan(&rel, v("0.2.0"), t).unwrap().kind, Kind::Full);
        let plan = plan(&rel, v("0.3.0"), t).unwrap();
        assert_eq!(
            (plan.kind, plan.asset.size),
            (Kind::Full, f.full_xz.len() as u64)
        );
        let dir = tempfile::tempdir().unwrap();
        let got = run(&rel, &f, dir.path()).await.unwrap();
        assert_eq!((got.kind, got.bytes), (Kind::Full, f.new));
    }

    #[tokio::test]
    async fn refuses_a_corrupted_patch() {
        let f = fixture();
        let t = target();
        let mut server = mockito::Server::new_async().await;
        let mut bad = f.patch_xz.clone();
        let mid = bad.len() / 2;
        bad[mid] ^= 1;
        let files = [(t.patch(v("0.3.0"), v("0.4.0")).leak() as &str, bad)];
        let rel = release(&mut server, &files, &sums_for(&f)).await;
        let dir = tempfile::tempdir().unwrap();
        let err = run(&rel, &f, dir.path()).await.unwrap_err();
        assert!(
            matches!(err, PrepareError::Verify(VerifyError::Download(_))),
            "{err:?}"
        );
        assert!(left_in(dir.path()).is_empty());
    }

    #[tokio::test]
    async fn refuses_a_wrong_bin_line() {
        let f = fixture();
        let t = target();
        let mut server = mockito::Server::new_async().await;
        let files = [(t.full(v("0.4.0")).leak() as &str, f.full_xz.clone())];
        let listed = line(&f.full_xz, &t.full(v("0.4.0"))) + &line(b"other", &t.bin(v("0.4.0")));
        let rel = release(&mut server, &files, &listed).await;
        let dir = tempfile::tempdir().unwrap();
        let err = run(&rel, &f, dir.path()).await.unwrap_err();
        assert!(
            matches!(err, PrepareError::Verify(VerifyError::Binary(_))),
            "{err:?}"
        );
        assert!(left_in(dir.path()).is_empty());
    }

    #[tokio::test]
    async fn refuses_an_unsigned_list() {
        let f = fixture();
        let t = target();
        let mut server = mockito::Server::new_async().await;
        let files = [(t.full(v("0.4.0")).leak() as &str, f.full_xz.clone())];
        let mut rel = release(&mut server, &files, &sums_for(&f)).await;
        rel.assets.retain(|a| a.name != SIGNATURE);
        let dir = tempfile::tempdir().unwrap();
        let err = run(&rel, &f, dir.path()).await.unwrap_err();
        assert!(
            matches!(err, PrepareError::Verify(VerifyError::Signature(_))),
            "{err:?}"
        );
    }

    #[tokio::test]
    async fn a_404_leaves_nothing() {
        let f = fixture();
        let t = target();
        let mut server = mockito::Server::new_async().await;
        let mut rel = release(&mut server, &[], &sums_for(&f)).await;
        rel.assets.push(Asset {
            name: t.full(v("0.4.0")),
            url: format!("{}/missing", server.url()),
            size: 1,
        });
        server
            .mock("GET", "/missing")
            .with_status(404)
            .create_async()
            .await;
        let dir = tempfile::tempdir().unwrap();
        let err = run(&rel, &f, dir.path()).await.unwrap_err();
        assert!(
            matches!(err, PrepareError::Download(ref e) if e.contains("404")),
            "{err:?}"
        );
        assert!(left_in(dir.path()).is_empty());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_read_only_folder_fails_cleanly() {
        use std::os::unix::fs::PermissionsExt;
        let f = fixture();
        let t = target();
        let mut server = mockito::Server::new_async().await;
        let files = [(t.full(v("0.4.0")).leak() as &str, f.full_xz.clone())];
        let rel = release(&mut server, &files, &sums_for(&f)).await;
        let dir = tempfile::tempdir().unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o555)).unwrap();
        let err = run(&rel, &f, dir.path()).await.unwrap_err();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(matches!(err, PrepareError::Io(_)), "{err:?}");
        assert!(left_in(dir.path()).is_empty());
    }
}
