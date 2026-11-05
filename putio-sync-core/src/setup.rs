//! First-run setup (feature 013): why setup is needed, folder checks, suggestions, and
//! applying the choices made in the web wizard or `putio-sync setup` to a [`Config`].
//!
//! Everything here is local; the Put.io account check, the device link and the dry run
//! live in the crates that talk to Put.io.

use crate::config::{ArrConfig, AuthConfig, AuthMethod, Config, DownloadConfig};
use crate::error::{CoreError, CoreResult};
use crate::path_utils::expand_path;
use crate::rules::{builtin, normalize_path, FolderRule};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

/// The Put.io app ID built into this binary, set at build time with
/// `PUTIO_SYNC_BUILTIN_APP_ID`. No release sets one yet, so without `[putio] app_id`
/// or `PUTIO_SYNC_APP_ID` the wizard asks for an app ID or a pasted token.
pub const BUILTIN_APP_ID: Option<&str> = option_env!("PUTIO_SYNC_BUILTIN_APP_ID");

/// Where to register a Put.io app, whose ID turns on the device link
pub const APP_REGISTER_URL: &str = "https://app.put.io/oauth";

/// Why the device link is off and how to turn it on, for the wizards and the doctor
pub const APP_ID_HELP: &str = "Linking with a code at put.io/link needs a Put.io app ID, and none is set. Register an app at https://app.put.io/oauth (account menu → API → Create App; any website and callback URL will do), then enter its ID in the setup wizard or Settings → Put.io, or set `[putio] app_id` in the config or PUTIO_SYNC_APP_ID. Until then, paste a token from put.io → Settings → API.";

/// Free space below this always warns (10 GiB)
pub const LOW_SPACE_BYTES: u64 = 10 * 1024 * 1024 * 1024;

/// Why the daemon is in setup mode
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum SetupReason {
    /// The config file does not exist
    NoConfig,
    /// The config file does not parse; the message names no file content
    InvalidConfig {
        /// What is wrong, with the line
        message: String,
    },
    /// No Put.io token in the config
    NoCredentials,
    /// Put.io refused the token (401)
    CredentialsRejected,
    /// The download folder does not exist and cannot be created
    FolderMissing {
        /// The folder
        path: String,
    },
    /// The download folder is not writable
    FolderNotWritable {
        /// The folder
        path: String,
    },
}

impl SetupReason {
    /// The reason in words, for the console and the log
    pub fn describe(&self) -> String {
        match self {
            SetupReason::NoConfig => "no config file".to_string(),
            SetupReason::InvalidConfig { message } => format!("config file is invalid: {message}"),
            SetupReason::NoCredentials => "no Put.io token".to_string(),
            SetupReason::CredentialsRejected => "Put.io token rejected".to_string(),
            SetupReason::FolderMissing { path } => {
                format!("download folder {path} does not exist and cannot be created")
            }
            SetupReason::FolderNotWritable { path } => {
                format!("download folder {path} is not writable")
            }
        }
    }
}

/// The config file as setup sees it: the parsed config (when it parses) and the reasons
/// found without asking Put.io
#[derive(Debug, Clone)]
pub struct LocalCheck {
    /// The parsed config, paths expanded; `None` when missing or invalid
    pub config: Option<Config>,
    /// Why setup is needed; empty when the file looks complete
    pub reasons: Vec<SetupReason>,
}

impl LocalCheck {
    /// The Put.io token, when the config has one
    pub fn putio_token(&self) -> Option<&str> {
        self.config.as_ref().and_then(putio_token)
    }
}

/// The Put.io token in `config`, when set (OAuth token auth only)
pub fn putio_token(config: &Config) -> Option<&str> {
    match config.auth.as_ref() {
        Some(AuthConfig {
            auth_method: AuthMethod::Token,
            token: Some(token),
            ..
        }) if !token.trim().is_empty() => Some(token.as_str()),
        _ => None,
    }
}

/// Read `path` and list why setup is needed, without asking Put.io. App-password auth
/// counts as configured (the daemon reports that it is unsupported, as before).
pub fn check_local(path: &Path) -> LocalCheck {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return LocalCheck {
                config: None,
                reasons: vec![SetupReason::NoConfig],
            }
        }
        Err(e) => {
            return LocalCheck {
                config: None,
                reasons: vec![SetupReason::InvalidConfig {
                    message: format!("cannot be read ({})", e.kind()),
                }],
            }
        }
    };
    let config = match parse(&contents) {
        Ok(config) => config,
        Err(message) => {
            return LocalCheck {
                config: None,
                reasons: vec![SetupReason::InvalidConfig { message }],
            }
        }
    };
    let mut reasons = Vec::new();
    let has_auth = matches!(
        config.auth.as_ref().map(|a| &a.auth_method),
        Some(AuthMethod::AppPassword)
    ) || putio_token(&config).is_some();
    if !has_auth {
        reasons.push(SetupReason::NoCredentials);
    }
    let target = &config.download.target_directory;
    let check = check_folder(target, None, false);
    if !check.usable() {
        let path = target.display().to_string();
        reasons.push(if check.exists {
            SetupReason::FolderNotWritable { path }
        } else {
            SetupReason::FolderMissing { path }
        });
    }
    LocalCheck {
        config: Some(config),
        reasons,
    }
}

/// Parse config text the way `Config::load` does, with an error that quotes no file
/// content (a token could be on the bad line)
fn parse(contents: &str) -> Result<Config, String> {
    let mut config: Config = toml::from_str(contents).map_err(|e| {
        let line = e.span().map(|span| {
            contents[..span.start.min(contents.len())]
                .matches('\n')
                .count()
                + 1
        });
        match line {
            Some(line) => format!("{} (line {line})", e.message()),
            None => e.message().to_string(),
        }
    })?;
    config
        .expand_paths()
        .map_err(|_| "a path cannot be expanded".to_string())?;
    Ok(config)
}

/// Something about a folder worth telling the user, without blocking
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "warning", rename_all = "snake_case")]
pub enum FolderWarning {
    /// Less free space than the account uses on Put.io
    LessThanPutioUsed {
        /// Free bytes here
        free: u64,
        /// Bytes used on Put.io
        used: u64,
    },
    /// Less than [`LOW_SPACE_BYTES`] free
    LowSpace {
        /// Free bytes here
        free: u64,
    },
    /// The daemon runs in a container: the path is inside it
    ContainerPath,
}

/// What a folder check found
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct FolderCheck {
    /// The folder, as the daemon sees it
    pub path: String,
    /// It exists
    pub exists: bool,
    /// This check created it
    pub created: bool,
    /// It is a directory
    pub is_dir: bool,
    /// A file can be created and removed in it
    pub writable: bool,
    /// Missing, but the nearest existing parent is writable
    pub can_create: bool,
    /// Free bytes, when the filesystem reports them
    pub free_bytes: Option<u64>,
    /// Total bytes, when the filesystem reports them
    pub total_bytes: Option<u64>,
    /// Non-blocking findings
    pub warnings: Vec<FolderWarning>,
    /// Why the folder cannot be used
    pub error: Option<String>,
}

impl FolderCheck {
    /// The folder works as a download or watch folder (or can be created as one)
    pub fn usable(&self) -> bool {
        (self.is_dir && self.writable) || (!self.exists && self.can_create)
    }
}

/// Check `path` for use as a download or watch folder. With `create`, a missing folder
/// is created first. `putio_used` (bytes used on Put.io) adds the space warning.
pub fn check_folder(path: &Path, putio_used: Option<u64>, create: bool) -> FolderCheck {
    let mut check = FolderCheck {
        path: path.display().to_string(),
        ..FolderCheck::default()
    };
    if !path.is_absolute() {
        check.error = Some("Use a full path, starting with /".to_string());
        return check;
    }
    if create && !path.exists() {
        match fs::create_dir_all(path) {
            Ok(()) => check.created = true,
            Err(e) => check.error = Some(format!("Cannot create the folder: {}", io_reason(&e))),
        }
    }
    check.exists = path.exists();
    check.is_dir = path.is_dir();
    let space_at = if check.exists {
        if !check.is_dir {
            check.error = Some("This is a file, not a folder".to_string());
            return check;
        }
        match probe_write(path) {
            Ok(()) => check.writable = true,
            Err(e) => {
                check.error = Some(format!("putio-sync cannot write here: {}", io_reason(&e)))
            }
        }
        Some(path.to_path_buf())
    } else if check.error.is_none() {
        match nearest_existing(path) {
            Some(parent) if parent.is_dir() && probe_write(&parent).is_ok() => {
                check.can_create = true;
                Some(parent)
            }
            Some(parent) => {
                check.error = Some(format!(
                    "The folder does not exist and {} is not writable",
                    parent.display()
                ));
                Some(parent)
            }
            None => {
                check.error = Some("The folder does not exist".to_string());
                None
            }
        }
    } else {
        None
    };
    if let Some(dir) = space_at {
        if let Ok((free, total)) = putio_sync_telemetry::gauges::disk_usage(&dir) {
            // Network shares sometimes report nothing; then the space is unknown
            if total > 0 {
                check.free_bytes = Some(free);
                check.total_bytes = Some(total);
            }
        }
    }
    if let Some(free) = check.free_bytes {
        if let Some(used) = putio_used.filter(|used| free < *used) {
            check
                .warnings
                .push(FolderWarning::LessThanPutioUsed { free, used });
        }
        if free < LOW_SPACE_BYTES {
            check.warnings.push(FolderWarning::LowSpace { free });
        }
    }
    if in_container() {
        check.warnings.push(FolderWarning::ContainerPath);
    }
    check
}

fn io_reason(e: &std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::PermissionDenied => "permission denied".to_string(),
        std::io::ErrorKind::ReadOnlyFilesystem => "read-only filesystem".to_string(),
        kind => kind.to_string(),
    }
}

/// Create and remove a probe file in `dir`
fn probe_write(dir: &Path) -> std::io::Result<()> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let probe = dir.join(format!(
        ".putio-sync-write-test-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)?;
    fs::remove_file(&probe)
}

fn nearest_existing(path: &Path) -> Option<PathBuf> {
    path.ancestors()
        .skip(1)
        .find(|p| p.exists())
        .map(Path::to_path_buf)
}

/// The daemon runs in a Docker or Podman container
pub fn in_container() -> bool {
    Path::new("/.dockerenv").exists()
        || Path::new("/run/.containerenv").exists()
        || std::env::var_os("container").is_some()
}

/// The environment variable that sets the Put.io app ID over the config
pub const APP_ID_VAR: &str = "PUTIO_SYNC_APP_ID";

/// Where the Put.io app ID in use comes from
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AppIdSource {
    /// `PUTIO_SYNC_APP_ID`
    Environment,
    /// `[putio] app_id`
    Config,
    /// [`BUILTIN_APP_ID`]
    BuiltIn,
}

/// The Put.io app ID for the device link and where it comes from: `PUTIO_SYNC_APP_ID`,
/// else `[putio] app_id`, else [`BUILTIN_APP_ID`]. Empty values count as unset.
pub fn app_id_with_source(config: &Config) -> Option<(String, AppIdSource)> {
    let set = |v: &str| Some(v.trim().to_string()).filter(|v| !v.is_empty());
    let env = std::env::var(APP_ID_VAR).ok();
    env.as_deref()
        .and_then(set)
        .map(|id| (id, AppIdSource::Environment))
        .or_else(|| {
            config
                .putio
                .app_id
                .as_deref()
                .and_then(set)
                .map(|id| (id, AppIdSource::Config))
        })
        .or_else(|| {
            BUILTIN_APP_ID
                .and_then(set)
                .map(|id| (id, AppIdSource::BuiltIn))
        })
}

/// The Put.io app ID for the device link; see [`app_id_with_source`]
pub fn resolve_app_id(config: &Config) -> Option<String> {
    app_id_with_source(config).map(|(id, _)| id)
}

/// The download folder to suggest: the configured one when it is not the built-in
/// default, else `/downloads` when it exists, else `putio-downloads` in the home folder
pub fn suggest_download_dir(config: Option<&Config>) -> PathBuf {
    suggest_download_dir_from(config, Path::new("/downloads"), dirs::home_dir())
}

fn suggest_download_dir_from(
    config: Option<&Config>,
    container_dir: &Path,
    home: Option<PathBuf>,
) -> PathBuf {
    let default = DownloadConfig::default().target_directory;
    let default_expanded = expand_path(&default).unwrap_or_else(|_| default.clone());
    if let Some(target) = config
        .map(|c| &c.download.target_directory)
        .filter(|t| **t != default && **t != default_expanded)
    {
        return target.clone();
    }
    if container_dir.is_dir() {
        return container_dir.to_path_buf();
    }
    match home {
        Some(home) => home.join("putio-downloads"),
        None => default_expanded,
    }
}

/// The watch folder to suggest: the first configured one, else `/watches` when it exists
pub fn suggest_watch_dir(config: Option<&Config>) -> Option<PathBuf> {
    suggest_watch_dir_from(config, Path::new("/watches"))
}

fn suggest_watch_dir_from(config: Option<&Config>, container_dir: &Path) -> Option<PathBuf> {
    config
        .and_then(|c| c.watch.paths.first().cloned())
        .or_else(|| container_dir.is_dir().then(|| container_dir.to_path_buf()))
}

/// What to do with one top-level Put.io folder
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct FolderChoice {
    /// The folder, `/Name` or `Name`
    pub path: String,
    /// Preset for it; `None` with `skip` false keeps the account-wide one
    #[serde(default)]
    pub preset: Option<String>,
    /// Download nothing from it
    #[serde(default)]
    pub skip: bool,
}

/// What the wizard or `putio-sync setup` collected
#[derive(Clone, Default)]
pub struct SetupChoices {
    /// Put.io OAuth token
    pub putio_token: Option<String>,
    /// Download folder
    pub download_dir: Option<PathBuf>,
    /// Account-wide preset
    pub preset: Option<String>,
    /// Per top-level folder choices
    pub folders: Vec<FolderChoice>,
    /// `Some(Some(dir))` turns watching on with `dir`; `Some(None)` turns it off
    pub watch_dir: Option<Option<PathBuf>>,
    /// Turn metrics on or off
    pub metrics: Option<bool>,
    /// Metrics listen address
    pub metrics_bind: Option<String>,
    /// API token to save (when setup created one)
    pub api_token: Option<String>,
    /// `[arr]` after the Radarr/Sonarr step
    pub arr: Option<ArrConfig>,
}

impl std::fmt::Debug for SetupChoices {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let set = |v: bool| if v { "<set>" } else { "<unset>" };
        f.debug_struct("SetupChoices")
            .field("putio_token", &set(self.putio_token.is_some()))
            .field("download_dir", &self.download_dir)
            .field("preset", &self.preset)
            .field("folders", &self.folders)
            .field("watch_dir", &self.watch_dir)
            .field("metrics", &self.metrics)
            .field("metrics_bind", &self.metrics_bind)
            .field("api_token", &set(self.api_token.is_some()))
            .field("arr", &set(self.arr.is_some()))
            .finish()
    }
}

impl SetupChoices {
    /// What must still be chosen before saving: `putio`, `download_dir`
    pub fn missing(&self) -> Vec<&'static str> {
        let mut missing = Vec::new();
        if self.putio_token.is_none() {
            missing.push("putio");
        }
        if self.download_dir.is_none() {
            missing.push("download_dir");
        }
        missing
    }

    /// Write the choices into `config`, keeping every other setting. Folder choices
    /// update the rule for that folder (keeping its other fields) or add one; a choice
    /// with no preset and no skip leaves the folder alone. The result is validated.
    pub fn apply(&self, config: &mut Config) -> CoreResult<()> {
        if let Some(token) = &self.putio_token {
            config.auth = Some(AuthConfig::with_token(token.clone()));
        }
        if let Some(dir) = &self.download_dir {
            config.download.target_directory = dir.clone();
        }
        if let Some(preset) = &self.preset {
            config.download.preset = Some(preset.clone());
        }
        for choice in &self.folders {
            if choice.preset.is_none() && !choice.skip {
                continue;
            }
            let path = normalize_path(&choice.path)?;
            let existing = config
                .download
                .rules
                .iter_mut()
                .find(|r| normalize_path(&r.path).ok().as_deref() == Some(path.as_str()));
            let rule = match existing {
                Some(rule) => rule,
                None => {
                    config.download.rules.push(FolderRule {
                        path: path.clone(),
                        ..FolderRule::default()
                    });
                    config.download.rules.last_mut().expect("just pushed")
                }
            };
            if choice.skip {
                rule.skip = Some(true);
            } else {
                rule.skip = None;
                rule.preset = choice.preset.clone();
            }
        }
        match &self.watch_dir {
            Some(Some(dir)) => {
                config.watch.enabled = true;
                config.watch.paths = vec![dir.clone()];
            }
            Some(None) => config.watch.enabled = false,
            None => {}
        }
        if let Some(on) = self.metrics {
            config.metrics.enabled = on;
        }
        if let Some(bind) = &self.metrics_bind {
            config.metrics.bind = crate::listen::BindList::from_env(bind);
        }
        if let Some(token) = &self.api_token {
            config.api.auth_token = Some(token.clone());
        }
        if let Some(arr) = &self.arr {
            config.arr = arr.clone();
        }
        crate::rules::validate(&config.download.rules, &config.download.presets)?;
        crate::rules::validate_root_patterns(&config.download)?;
        crate::rules::validate_root_preset(
            config.download.preset.as_deref(),
            &config.download.presets,
        )?;
        config.resolve_metrics()?;
        Ok(())
    }
}

/// The presets the wizard lists: built-ins in UI order, then `[download.presets]` ones,
/// as `(name, label)`
pub fn preset_choices(download: &DownloadConfig) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = crate::rules::BUILTIN_PRESETS
        .iter()
        .map(|p| (p.name.to_string(), p.label.to_string()))
        .collect();
    for name in download.presets.keys() {
        if builtin(name).is_none() {
            out.push((name.clone(), name.clone()));
        }
    }
    out
}

/// Check that `name` is a preset the account can use
pub fn check_preset(download: &DownloadConfig, name: &str) -> CoreResult<()> {
    crate::rules::validate_root_preset(Some(name), &download.presets)
        .map_err(|_| CoreError::ConfigError(format!("Unknown preset '{name}'")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn write_config(dir: &Path, config: &Config) -> PathBuf {
        let path = dir.join("config.toml");
        config.save(&path).unwrap();
        path
    }

    fn complete(dir: &Path) -> Config {
        let mut config = Config {
            auth: Some(AuthConfig::with_token("tok".to_string())),
            ..Config::default()
        };
        config.download.target_directory = dir.join("dl");
        config
    }

    #[test]
    fn missing_config_needs_setup() {
        let dir = tempfile::tempdir().unwrap();
        let check = check_local(&dir.path().join("none.toml"));
        assert_eq!(check.reasons, vec![SetupReason::NoConfig]);
        assert!(check.config.is_none());
    }

    #[test]
    fn invalid_config_names_the_line_not_the_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "[auth]\ntoken = \"secret-token\"\nbroken = = 1\n").unwrap();
        let check = check_local(&path);
        match &check.reasons[..] {
            [SetupReason::InvalidConfig { message }] => {
                assert!(message.contains("line 3"), "{message}");
                assert!(!message.contains("secret-token"), "{message}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn no_token_needs_setup() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = complete(dir.path());
        config.auth = None;
        let check = check_local(&write_config(dir.path(), &config));
        assert_eq!(check.reasons, vec![SetupReason::NoCredentials]);
        assert!(check.config.is_some());
        assert_eq!(check.putio_token(), None);
    }

    #[test]
    fn complete_config_needs_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let check = check_local(&write_config(dir.path(), &complete(dir.path())));
        assert!(check.reasons.is_empty(), "{:?}", check.reasons);
        assert_eq!(check.putio_token(), Some("tok"));
    }

    #[cfg(unix)]
    #[test]
    fn unwritable_folder_needs_setup() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let locked = dir.path().join("locked");
        fs::create_dir(&locked).unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o555)).unwrap();
        if probe_write(&locked).is_ok() {
            return; // running as root
        }
        let mut config = complete(dir.path());
        config.download.target_directory = locked.clone();
        let check = check_local(&write_config(dir.path(), &config));
        assert_eq!(
            check.reasons,
            vec![SetupReason::FolderNotWritable {
                path: locked.display().to_string()
            }]
        );
        config.download.target_directory = locked.join("below");
        let check = check_local(&write_config(dir.path(), &config));
        assert!(matches!(
            check.reasons[..],
            [SetupReason::FolderMissing { .. }]
        ));
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn folder_check_reports_space_and_creates() {
        let dir = tempfile::tempdir().unwrap();
        let check = check_folder(dir.path(), None, false);
        assert!(check.usable() && check.writable && check.is_dir);
        assert!(check.total_bytes.is_some());

        let missing = dir.path().join("a/b");
        let check = check_folder(&missing, None, false);
        assert!(!check.exists && check.can_create && check.usable());
        assert!(!missing.exists());
        let check = check_folder(&missing, None, true);
        assert!(check.created && check.is_dir && check.writable);

        let check = check_folder(&dir.path().join("a/b"), Some(u64::MAX), false);
        assert!(check
            .warnings
            .iter()
            .any(|w| matches!(w, FolderWarning::LessThanPutioUsed { .. })));
        // No probe file is left behind
        assert_eq!(fs::read_dir(&missing).unwrap().count(), 0);
    }

    #[test]
    fn folder_check_refuses_files_and_relative_paths() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("f");
        fs::write(&file, "x").unwrap();
        let check = check_folder(&file, None, false);
        assert!(!check.usable());
        assert!(check.error.unwrap().contains("not a folder"));
        let check = check_folder(Path::new("relative/dir"), None, false);
        assert!(!check.usable());
        assert!(check.error.unwrap().contains("full path"));
    }

    #[test]
    fn suggestions() {
        let dir = tempfile::tempdir().unwrap();
        let none = dir.path().join("nope");
        let home = Some(dir.path().join("home"));
        assert_eq!(
            suggest_download_dir_from(None, &none, home.clone()),
            dir.path().join("home/putio-downloads")
        );
        assert_eq!(
            suggest_download_dir_from(None, dir.path(), home.clone()),
            dir.path()
        );
        let mut config = Config::default();
        assert_eq!(
            suggest_download_dir_from(Some(&config), &none, home.clone()),
            dir.path().join("home/putio-downloads")
        );
        config.download.target_directory = PathBuf::from("/data/putio");
        assert_eq!(
            suggest_download_dir_from(Some(&config), dir.path(), home),
            PathBuf::from("/data/putio")
        );

        assert_eq!(suggest_watch_dir_from(None, &none), None);
        assert_eq!(
            suggest_watch_dir_from(None, dir.path()),
            Some(dir.path().to_path_buf())
        );
        config.watch.paths = vec![PathBuf::from("/w")];
        assert_eq!(
            suggest_watch_dir_from(Some(&config), dir.path()),
            Some(PathBuf::from("/w"))
        );
    }

    #[test]
    fn app_id_from_config() {
        let mut config = Config::default();
        config.putio.app_id = Some(" 42 ".to_string());
        // The environment wins over the config; tests leave it unset
        if std::env::var(APP_ID_VAR).is_err() {
            assert_eq!(
                app_id_with_source(&config),
                Some(("42".to_string(), AppIdSource::Config))
            );
        }
        config.putio.app_id = Some(String::new());
        // Falls through to the built-in, which tests leave unset
        if std::env::var(APP_ID_VAR).is_err() && BUILTIN_APP_ID.is_none() {
            assert_eq!(resolve_app_id(&config), None);
        }
    }

    #[test]
    fn apply_keeps_other_settings() {
        let mut config = Config::default();
        config.download.workers = 7;
        config.cleanup.enabled = true;
        config.download.rules = vec![FolderRule {
            path: "Movies".to_string(),
            delete_blocked: Some(true),
            ..FolderRule::default()
        }];
        let choices = SetupChoices {
            putio_token: Some("tok".to_string()),
            download_dir: Some(PathBuf::from("/dl")),
            preset: Some("no-binaries".to_string()),
            folders: vec![
                FolderChoice {
                    path: "/Movies".to_string(),
                    preset: Some("video".to_string()),
                    skip: false,
                },
                FolderChoice {
                    path: "/Junk".to_string(),
                    preset: None,
                    skip: true,
                },
                FolderChoice {
                    path: "/Other".to_string(),
                    preset: None,
                    skip: false,
                },
            ],
            watch_dir: Some(Some(PathBuf::from("/w"))),
            metrics: Some(true),
            metrics_bind: Some("127.0.0.1:19090".to_string()),
            api_token: Some("api".to_string()),
            arr: None,
        };
        assert!(choices.missing().is_empty());
        choices.apply(&mut config).unwrap();
        assert_eq!(putio_token(&config), Some("tok"));
        assert_eq!(config.download.target_directory, PathBuf::from("/dl"));
        assert_eq!(config.download.preset.as_deref(), Some("no-binaries"));
        assert_eq!(config.download.workers, 7);
        assert!(config.cleanup.enabled);
        assert_eq!(config.download.rules.len(), 2);
        let movies = &config.download.rules[0];
        assert_eq!(movies.preset.as_deref(), Some("video"));
        assert_eq!(movies.delete_blocked, Some(true));
        assert_eq!(config.download.rules[1].path, "Junk");
        assert_eq!(config.download.rules[1].skip, Some(true));
        assert!(config.watch.enabled);
        assert_eq!(config.watch.paths, vec![PathBuf::from("/w")]);
        assert!(config.metrics.enabled);
        assert_eq!(config.api.auth_token.as_deref(), Some("api"));
        let debug = format!("{choices:?}");
        assert!(
            !debug.contains("tok\"") && !debug.contains("\"api\""),
            "{debug}"
        );
    }

    #[test]
    fn apply_refuses_unknown_presets_and_bad_binds() {
        let mut config = Config::default();
        let choices = SetupChoices {
            preset: Some("comics".to_string()),
            ..SetupChoices::default()
        };
        assert!(choices.apply(&mut config).is_err());
        let choices = SetupChoices {
            metrics: Some(true),
            metrics_bind: Some("not an address".to_string()),
            ..SetupChoices::default()
        };
        assert!(choices.apply(&mut Config::default()).is_err());
        assert_eq!(
            SetupChoices::default().missing(),
            vec!["putio", "download_dir"]
        );
    }

    #[test]
    fn preset_list_has_builtins_then_user_presets() {
        let mut download = DownloadConfig::default();
        download
            .presets
            .insert("comics".to_string(), crate::rules::Preset::default());
        download
            .presets
            .insert("video".to_string(), crate::rules::Preset::default());
        let list = preset_choices(&download);
        assert_eq!(list[0].0, "video");
        assert_eq!(list.last().unwrap().0, "comics");
        assert_eq!(list.len(), crate::rules::BUILTIN_PRESETS.len() + 1);
        assert!(check_preset(&download, "comics").is_ok());
        assert!(check_preset(&download, "nope").is_err());
    }
}
