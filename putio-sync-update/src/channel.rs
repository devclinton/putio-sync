//! How putio-sync was installed, which decides how it is updated (research R10).
//!
//! Detection runs once at start, needs no network, and reads the system only through a
//! [`Probe`], so tests fake every channel. `PUTIO_SYNC_INSTALL=<kind>` overrides it for
//! installs the markers get wrong.

use crate::version::Version;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// The image the Dockerfiles bake in when no build argument names another
pub const DEFAULT_IMAGE: &str = "ghcr.io/devclinton/putio-sync";
const RELEASES: &str = "https://github.com/devclinton/putio-sync/releases";
const SETUP_KEY: &str = r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\{6F0B8D2A-6C1E-4F57-9C38-5B1D2E7A4C90}_is1";
const PKG_RECEIPT: &str = "/var/db/receipts/com.devclinton.putio-sync.plist";
const DPKG_LIST: &str = "/var/lib/dpkg/info/putio-sync.list";

/// How an update is applied
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum UpdateMode {
    /// Download, verify, swap the binary and restart
    Swap,
    /// Download and open the platform installer
    Installer,
    /// Show the channel's own update command
    Instructions,
}

/// An install channel
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Channel {
    Snap,
    Flatpak,
    Unraid,
    CasaOs {
        image: String,
    },
    Umbrel,
    /// `variant` is the image's tag family (`latest`, `dhi`, …); `None` when the image
    /// didn't say, so the right pull can't be named
    Container {
        image: String,
        variant: Option<String>,
    },
    Homebrew,
    Deb,
    Winget,
    MacPkg,
    WindowsSetup,
    GithubArchive,
    /// A dev build, or nothing matched
    Unknown,
}

/// Reads a file, `None` when it can't
pub type ReadFile = Box<dyn Fn(&Path) -> Option<String>>;
/// Reads a registry value by key and name
pub type Registry = Box<dyn Fn(&str, &str) -> Option<String>>;

/// What the system looks like to detection. [`Probe::system`] is the real one.
pub struct Probe {
    pub env: HashMap<String, String>,
    /// The running binary, symlinks resolved
    pub exe: PathBuf,
    /// Built by the release workflow (`PUTIO_SYNC_GIT_SHA` set at compile time)
    pub release_build: bool,
    pub file_exists: Box<dyn Fn(&Path) -> bool>,
    pub read_file: ReadFile,
    /// `(key, value name)` → the registry value; always `None` off Windows
    pub registry: Registry,
}

impl Probe {
    /// The running process and the real system
    pub fn system() -> Self {
        let exe = std::env::current_exe()
            .and_then(|p| p.canonicalize())
            .unwrap_or_default();
        Self {
            env: std::env::vars().collect(),
            exe,
            release_build: option_env!("PUTIO_SYNC_GIT_SHA").is_some_and(|s| s != "unknown"),
            file_exists: Box::new(|p| p.exists()),
            read_file: Box::new(|p| std::fs::read_to_string(p).ok()),
            registry: Box::new(registry_value),
        }
    }

    fn var(&self, name: &str) -> Option<&str> {
        self.env
            .get(name)
            .map(String::as_str)
            .filter(|v| !v.is_empty())
    }

    fn in_container(&self) -> bool {
        self.var("PUTIO_SYNC_IMAGE_VARIANT").is_some()
            || (self.file_exists)(Path::new("/.dockerenv"))
            || (self.read_file)(Path::new("/proc/1/cgroup")).is_some_and(|c| {
                ["docker", "containerd", "kubepods"]
                    .iter()
                    .any(|m| c.contains(m))
            })
    }

    fn image(&self) -> String {
        self.var("PUTIO_SYNC_IMAGE")
            .unwrap_or(DEFAULT_IMAGE)
            .to_string()
    }

    fn container(&self) -> Channel {
        Channel::Container {
            image: self.image(),
            variant: self.var("PUTIO_SYNC_IMAGE_VARIANT").map(str::to_string),
        }
    }

    /// The exe path with `/` separators, so Windows paths match on any host
    fn exe_slashed(&self) -> String {
        self.exe.to_string_lossy().replace('\\', "/")
    }
}

#[cfg(windows)]
fn registry_value(key: &str, name: &str) -> Option<String> {
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};
    let sub = key.strip_prefix(r"HKLM\")?;
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let (sub, name) = (wide(sub), wide(name));
    let mut buf = vec![0u16; 1024];
    let mut len = (buf.len() * 2) as u32;
    // SAFETY: the strings are NUL-terminated and `len` is the buffer's size in bytes
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            sub.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buf.as_mut_ptr().cast(),
            &mut len,
        )
    };
    if status != 0 {
        return None;
    }
    let chars = (len as usize / 2).saturating_sub(1);
    Some(String::from_utf16_lossy(&buf[..chars]))
}

#[cfg(not(windows))]
fn registry_value(_key: &str, _name: &str) -> Option<String> {
    None
}

/// How to update a managed install (contracts/api.md `available.instruction`)
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Instruction {
    /// Run these, in order
    pub commands: Vec<String>,
    /// Instead of the commands, for a pinned version
    pub alternatives: Vec<String>,
    pub steps: Option<String>,
    pub link: Option<String>,
}

/// The `.deb` architecture name of this build
pub fn deb_arch() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "amd64"
    }
}

impl Channel {
    /// Work out the channel of the running binary
    pub fn detect() -> Self {
        Self::detect_with(&Probe::system())
    }

    /// The first channel in research R10's order whose marker matches
    pub fn detect_with(probe: &Probe) -> Self {
        if let Some(kind) = probe.var("PUTIO_SYNC_INSTALL") {
            if let Some(channel) = Self::from_kind(kind, probe) {
                return channel;
            }
            tracing::warn!(
                kind,
                "PUTIO_SYNC_INSTALL names no install channel; detecting"
            );
        }
        if probe.var("SNAP").is_some() && probe.var("SNAP_NAME") == Some("putio-sync") {
            return Self::Snap;
        }
        if probe.var("FLATPAK_ID").is_some() {
            return Self::Flatpak;
        }
        if probe.in_container() {
            return match probe.var("HOST_OS") {
                Some(os) if os.eq_ignore_ascii_case("unraid") => Self::Unraid,
                _ => probe.container(),
            };
        }
        let exe = probe.exe_slashed();
        if exe.contains("/Cellar/putio-sync/") {
            return Self::Homebrew;
        }
        if exe == "/usr/bin/putio-sync"
            && (probe.read_file)(Path::new(DPKG_LIST))
                .is_some_and(|l| l.lines().any(|line| line == exe))
        {
            return Self::Deb;
        }
        if exe
            .to_ascii_lowercase()
            .contains("/microsoft/winget/packages/putio.putio-sync_")
        {
            return Self::Winget;
        }
        if exe == "/usr/local/bin/putio-sync" && (probe.file_exists)(Path::new(PKG_RECEIPT)) {
            return Self::MacPkg;
        }
        if let Some(location) = (probe.registry)(SETUP_KEY, "InstallLocation") {
            let folder = exe.rsplit_once('/').map_or("", |(dir, _)| dir);
            let location = location.replace('\\', "/");
            if !folder.is_empty()
                && location
                    .trim_end_matches('/')
                    .eq_ignore_ascii_case(folder.trim_end_matches('/'))
            {
                return Self::WindowsSetup;
            }
        }
        if probe.release_build {
            Self::GithubArchive
        } else {
            Self::Unknown
        }
    }

    fn from_kind(kind: &str, probe: &Probe) -> Option<Self> {
        Some(match kind.to_ascii_lowercase().as_str() {
            "snap" => Self::Snap,
            "flatpak" => Self::Flatpak,
            "unraid" => Self::Unraid,
            "casaos" => Self::CasaOs {
                image: probe.image(),
            },
            "umbrel" => Self::Umbrel,
            "container" => probe.container(),
            "homebrew" => Self::Homebrew,
            "deb" => Self::Deb,
            "winget" => Self::Winget,
            "pkg" => Self::MacPkg,
            "setup" => Self::WindowsSetup,
            "archive" => Self::GithubArchive,
            "unknown" => Self::Unknown,
            _ => return None,
        })
    }

    /// The `kind` the API and `GET /status` report, and `PUTIO_SYNC_INSTALL` accepts
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Snap => "snap",
            Self::Flatpak => "flatpak",
            Self::Unraid => "unraid",
            Self::CasaOs { .. } => "casaos",
            Self::Umbrel => "umbrel",
            Self::Container { .. } => "container",
            Self::Homebrew => "homebrew",
            Self::Deb => "deb",
            Self::Winget => "winget",
            Self::MacPkg => "pkg",
            Self::WindowsSetup => "setup",
            Self::GithubArchive => "archive",
            Self::Unknown => "unknown",
        }
    }

    /// A name for people
    pub fn label(&self) -> String {
        match self {
            Self::Snap => "Snap".into(),
            Self::Flatpak => "Flatpak".into(),
            Self::Unraid => "Unraid".into(),
            Self::CasaOs { .. } => "CasaOS".into(),
            Self::Umbrel => "Umbrel".into(),
            Self::Container {
                variant: Some(v), ..
            } => format!("Docker ({v} image)"),
            Self::Container { variant: None, .. } => "Docker".into(),
            Self::Homebrew => "Homebrew".into(),
            Self::Deb => ".deb package".into(),
            Self::Winget => "winget".into(),
            Self::MacPkg => "macOS installer".into(),
            Self::WindowsSetup => "Windows installer".into(),
            Self::GithubArchive => "GitHub download".into(),
            Self::Unknown => "Unknown install".into(),
        }
    }

    /// How this channel is updated
    pub fn mode(&self) -> UpdateMode {
        match self {
            Self::GithubArchive => UpdateMode::Swap,
            Self::MacPkg | Self::WindowsSetup => UpdateMode::Installer,
            _ => UpdateMode::Instructions,
        }
    }

    /// How to get from the running version to `to`; `arch` is the `.deb` architecture
    pub fn instruction(&self, to: &Version, arch: &str) -> Instruction {
        let one = |command: &str| Instruction {
            commands: vec![command.to_string()],
            ..Default::default()
        };
        match self {
            Self::Snap => one("sudo snap refresh putio-sync"),
            Self::Flatpak => one("flatpak update com.putio.PutioSync"),
            Self::Homebrew => one("brew upgrade putio-sync"),
            Self::Winget => one("winget upgrade putio.putio-sync"),
            Self::Deb => {
                let file = format!("putio-sync_{to}_{arch}.deb");
                Instruction {
                    commands: vec![format!("sudo dpkg -i {file}")],
                    link: Some(format!("{RELEASES}/download/v{to}/{file}")),
                    ..Default::default()
                }
            }
            Self::Container {
                image,
                variant: Some(variant),
            } => {
                let pinned = if variant == "latest" {
                    format!("{image}:{to}")
                } else {
                    format!("{image}:{variant}-{to}")
                };
                Instruction {
                    commands: vec![format!("docker pull {image}:{variant}")],
                    alternatives: vec![format!("docker pull {pinned}")],
                    steps: Some("Recreate the container after the pull.".into()),
                    link: None,
                }
            }
            Self::Container { variant: None, .. } => Instruction {
                steps: Some(
                    "Pull the newer putio-sync image the way you installed it, then recreate the container."
                        .into(),
                ),
                ..Default::default()
            },
            Self::Unraid => Instruction {
                steps: Some(
                    "In Unraid's Docker tab, choose Check for updates, then apply update for putio-sync."
                        .into(),
                ),
                ..Default::default()
            },
            Self::CasaOs { image } => Instruction {
                alternatives: vec![format!("{image}:{to}")],
                steps: Some(
                    "Update putio-sync from the CasaOS dashboard, or set the app's image tag to the new version."
                        .into(),
                ),
                ..Default::default()
            },
            Self::Umbrel => Instruction {
                steps: Some("Update putio-sync from the Umbrel App Store.".into()),
                ..Default::default()
            },
            Self::MacPkg | Self::WindowsSetup | Self::GithubArchive | Self::Unknown => {
                Instruction {
                    link: Some(format!("{RELEASES}/tag/v{to}")),
                    ..Default::default()
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// A probe with nothing set: no env, no files, no registry, a dev build
    fn probe(exe: &str) -> Probe {
        Probe {
            env: HashMap::new(),
            exe: PathBuf::from(exe),
            release_build: false,
            file_exists: Box::new(|_| false),
            read_file: Box::new(|_| None),
            registry: Box::new(|_, _| None),
        }
    }

    fn with_env(mut p: Probe, vars: &[(&str, &str)]) -> Probe {
        for (k, v) in vars {
            p.env.insert((*k).into(), (*v).into());
        }
        p
    }

    fn with_files(mut p: Probe, files: &[(&'static str, &'static str)]) -> Probe {
        let files: HashMap<&str, &str> = files.iter().copied().collect();
        let names: HashSet<String> = files.keys().map(|k| k.to_string()).collect();
        p.file_exists = Box::new(move |path| names.contains(path.to_str().unwrap()));
        p.read_file =
            Box::new(move |path| files.get(path.to_str().unwrap()).map(|s| s.to_string()));
        p
    }

    fn v(s: &str) -> Version {
        s.parse().unwrap()
    }

    #[test]
    fn snap() {
        let p = with_env(
            probe("/snap/putio-sync/12/bin/putio-sync"),
            &[("SNAP", "/snap/putio-sync/12"), ("SNAP_NAME", "putio-sync")],
        );
        assert_eq!(Channel::detect_with(&p), Channel::Snap);
        // Another snap's environment isn't ours
        let p = with_env(
            probe("/x"),
            &[("SNAP", "/snap/other/1"), ("SNAP_NAME", "other")],
        );
        assert_eq!(Channel::detect_with(&p), Channel::Unknown);
    }

    #[test]
    fn flatpak() {
        let p = with_env(
            probe("/app/bin/putio-sync"),
            &[("FLATPAK_ID", "com.putio.PutioSync")],
        );
        assert_eq!(Channel::detect_with(&p), Channel::Flatpak);
    }

    #[test]
    fn unraid() {
        let p = with_files(
            with_env(probe("/usr/local/bin/putio-sync"), &[("HOST_OS", "Unraid")]),
            &[("/.dockerenv", "")],
        );
        assert_eq!(Channel::detect_with(&p), Channel::Unraid);
        // HOST_OS outside a container means nothing
        let p = with_env(probe("/x"), &[("HOST_OS", "Unraid")]);
        assert_eq!(Channel::detect_with(&p), Channel::Unknown);
    }

    #[test]
    fn casaos_and_umbrel() {
        let p = with_env(
            probe("/x"),
            &[
                ("PUTIO_SYNC_INSTALL", "casaos"),
                ("PUTIO_SYNC_IMAGE_VARIANT", "latest"),
            ],
        );
        assert_eq!(
            Channel::detect_with(&p),
            Channel::CasaOs {
                image: DEFAULT_IMAGE.into()
            }
        );
        let p = with_env(probe("/x"), &[("PUTIO_SYNC_INSTALL", "umbrel")]);
        assert_eq!(Channel::detect_with(&p), Channel::Umbrel);
    }

    #[test]
    fn container_with_variant() {
        let p = with_env(
            probe("/usr/local/bin/putio-sync"),
            &[
                ("PUTIO_SYNC_IMAGE_VARIANT", "dhi"),
                ("PUTIO_SYNC_IMAGE", "ghcr.io/someone/putio-sync"),
            ],
        );
        assert_eq!(
            Channel::detect_with(&p),
            Channel::Container {
                image: "ghcr.io/someone/putio-sync".into(),
                variant: Some("dhi".into())
            }
        );
        assert_eq!(Channel::detect_with(&p).label(), "Docker (dhi image)");
    }

    #[test]
    fn container_from_dockerenv_alone() {
        let p = with_files(probe("/usr/local/bin/putio-sync"), &[("/.dockerenv", "")]);
        assert_eq!(
            Channel::detect_with(&p),
            Channel::Container {
                image: DEFAULT_IMAGE.into(),
                variant: None
            }
        );
    }

    #[test]
    fn container_from_cgroup() {
        let p = with_files(
            probe("/app/putio-sync"),
            &[("/proc/1/cgroup", "0::/kubepods/besteffort/pod1234\n")],
        );
        assert!(matches!(
            Channel::detect_with(&p),
            Channel::Container { variant: None, .. }
        ));
        // A host's own cgroup isn't a container
        let p = with_files(
            probe("/app/putio-sync"),
            &[("/proc/1/cgroup", "0::/init.scope\n")],
        );
        assert_eq!(Channel::detect_with(&p), Channel::Unknown);
    }

    #[test]
    fn homebrew() {
        let p = probe("/opt/homebrew/Cellar/putio-sync/0.3.0/bin/putio-sync");
        assert_eq!(Channel::detect_with(&p), Channel::Homebrew);
    }

    #[test]
    fn deb() {
        let p = with_files(
            probe("/usr/bin/putio-sync"),
            &[(DPKG_LIST, "/.\n/usr\n/usr/bin\n/usr/bin/putio-sync\n")],
        );
        assert_eq!(Channel::detect_with(&p), Channel::Deb);
        // /usr/bin without dpkg's list is someone's own copy
        let p = probe("/usr/bin/putio-sync");
        assert_eq!(Channel::detect_with(&p), Channel::Unknown);
    }

    #[test]
    fn winget() {
        let p = probe(
            r"C:\Users\x\AppData\Local\Microsoft\WinGet\Packages\putio.putio-sync_Microsoft.Winget.Source_8wekyb3d8bbwe\putio-sync.exe",
        );
        assert_eq!(Channel::detect_with(&p), Channel::Winget);
    }

    #[test]
    fn mac_pkg() {
        let p = with_files(probe("/usr/local/bin/putio-sync"), &[(PKG_RECEIPT, "")]);
        assert_eq!(Channel::detect_with(&p), Channel::MacPkg);
        let p = probe("/usr/local/bin/putio-sync");
        assert_eq!(Channel::detect_with(&p), Channel::Unknown);
    }

    #[test]
    fn windows_setup() {
        let mut p = probe(r"C:\Program Files\putio-sync\putio-sync.exe");
        p.registry = Box::new(|key, name| {
            (key == SETUP_KEY && name == "InstallLocation")
                .then(|| r"C:\Program Files\putio-sync\".to_string())
        });
        assert_eq!(Channel::detect_with(&p), Channel::WindowsSetup);
        // A copy elsewhere on a machine that also has the installer isn't the installed one
        p.exe = PathBuf::from(r"C:\Users\x\Downloads\putio-sync.exe");
        assert_eq!(Channel::detect_with(&p), Channel::Unknown);
    }

    #[test]
    fn github_archive_and_unknown() {
        let mut p = probe("/home/x/bin/putio-sync");
        assert_eq!(Channel::detect_with(&p), Channel::Unknown);
        p.release_build = true;
        assert_eq!(Channel::detect_with(&p), Channel::GithubArchive);
    }

    #[test]
    fn order() {
        // Snap wins over a dpkg-listed /usr/bin path
        let p = with_files(
            with_env(
                probe("/usr/bin/putio-sync"),
                &[("SNAP", "/snap/putio-sync/1"), ("SNAP_NAME", "putio-sync")],
            ),
            &[(DPKG_LIST, "/usr/bin/putio-sync\n")],
        );
        assert_eq!(Channel::detect_with(&p), Channel::Snap);
        // A container wins over Homebrew-looking paths
        let p = with_files(
            probe("/home/linuxbrew/.linuxbrew/Cellar/putio-sync/0.3.0/bin/putio-sync"),
            &[("/.dockerenv", "")],
        );
        assert!(matches!(
            Channel::detect_with(&p),
            Channel::Container { .. }
        ));
    }

    #[test]
    fn install_overrides_every_kind() {
        for kind in [
            "snap",
            "flatpak",
            "unraid",
            "casaos",
            "umbrel",
            "container",
            "homebrew",
            "deb",
            "winget",
            "pkg",
            "setup",
            "archive",
            "unknown",
        ] {
            // Markers for another channel lose to the override
            let p = with_env(
                probe("/opt/homebrew/Cellar/putio-sync/0.3.0/bin/putio-sync"),
                &[("PUTIO_SYNC_INSTALL", kind), ("FLATPAK_ID", "x")],
            );
            assert_eq!(Channel::detect_with(&p).kind(), kind);
        }
        // An unknown kind falls back to detection
        let p = with_env(
            probe("/x"),
            &[("PUTIO_SYNC_INSTALL", "nix"), ("FLATPAK_ID", "x")],
        );
        assert_eq!(Channel::detect_with(&p), Channel::Flatpak);
    }

    #[test]
    fn modes() {
        assert_eq!(Channel::GithubArchive.mode(), UpdateMode::Swap);
        assert_eq!(Channel::MacPkg.mode(), UpdateMode::Installer);
        assert_eq!(Channel::WindowsSetup.mode(), UpdateMode::Installer);
        for c in [
            Channel::Snap,
            Channel::Flatpak,
            Channel::Unraid,
            Channel::Umbrel,
            Channel::Homebrew,
            Channel::Deb,
            Channel::Winget,
            Channel::Unknown,
            Channel::CasaOs { image: "i".into() },
            Channel::Container {
                image: "i".into(),
                variant: None,
            },
        ] {
            assert_eq!(c.mode(), UpdateMode::Instructions, "{c:?}");
        }
    }

    #[test]
    fn one_command_instructions() {
        let to = v("0.4.0");
        for (c, cmd) in [
            (Channel::Snap, "sudo snap refresh putio-sync"),
            (Channel::Flatpak, "flatpak update com.putio.PutioSync"),
            (Channel::Homebrew, "brew upgrade putio-sync"),
            (Channel::Winget, "winget upgrade putio.putio-sync"),
        ] {
            assert_eq!(c.instruction(&to, "amd64").commands, vec![cmd.to_string()]);
        }
    }

    #[test]
    fn deb_instruction() {
        let i = Channel::Deb.instruction(&v("0.4.0"), "arm64");
        assert_eq!(i.link.as_deref(), Some("https://github.com/devclinton/putio-sync/releases/download/v0.4.0/putio-sync_0.4.0_arm64.deb"));
        assert_eq!(
            i.commands,
            vec!["sudo dpkg -i putio-sync_0.4.0_arm64.deb".to_string()]
        );
    }

    #[test]
    fn container_instructions() {
        let to = v("0.4.0");
        let latest = Channel::Container {
            image: DEFAULT_IMAGE.into(),
            variant: Some("latest".into()),
        }
        .instruction(&to, "amd64");
        assert_eq!(
            latest.commands,
            vec!["docker pull ghcr.io/devclinton/putio-sync:latest".to_string()]
        );
        assert_eq!(
            latest.alternatives,
            vec!["docker pull ghcr.io/devclinton/putio-sync:0.4.0".to_string()]
        );
        assert_eq!(
            latest.steps.as_deref(),
            Some("Recreate the container after the pull.")
        );

        let dhi = Channel::Container {
            image: DEFAULT_IMAGE.into(),
            variant: Some("dhi-ffmpeg".into()),
        }
        .instruction(&to, "amd64");
        assert_eq!(
            dhi.commands,
            vec!["docker pull ghcr.io/devclinton/putio-sync:dhi-ffmpeg".to_string()]
        );
        assert_eq!(
            dhi.alternatives,
            vec!["docker pull ghcr.io/devclinton/putio-sync:dhi-ffmpeg-0.4.0".to_string()]
        );

        let plain = Channel::Container {
            image: DEFAULT_IMAGE.into(),
            variant: None,
        }
        .instruction(&to, "amd64");
        assert!(plain.commands.is_empty());
        assert_eq!(plain.steps.as_deref(), Some("Pull the newer putio-sync image the way you installed it, then recreate the container."));
    }

    #[test]
    fn appliance_instructions() {
        let to = v("0.4.0");
        assert!(Channel::Unraid
            .instruction(&to, "amd64")
            .steps
            .unwrap()
            .contains("Check for updates"));
        assert!(Channel::Umbrel
            .instruction(&to, "amd64")
            .steps
            .unwrap()
            .contains("Umbrel App Store"));
        let casa = Channel::CasaOs {
            image: DEFAULT_IMAGE.into(),
        }
        .instruction(&to, "amd64");
        assert!(casa.steps.unwrap().contains("CasaOS dashboard"));
        assert_eq!(
            casa.alternatives,
            vec!["ghcr.io/devclinton/putio-sync:0.4.0".to_string()]
        );
    }

    #[test]
    fn unknown_gets_only_the_release_link() {
        let i = Channel::Unknown.instruction(&v("0.4.0"), "amd64");
        assert_eq!(
            i,
            Instruction {
                link: Some("https://github.com/devclinton/putio-sync/releases/tag/v0.4.0".into()),
                ..Default::default()
            }
        );
    }
}
