//! Configuration assistant (feature 015)
//!
//! [`risks`] lists settings that are valid together but can lose data or cause trouble,
//! such as deleting from Put.io while the download folder is in a temporary folder.
//! [`diff`] lists the settings that differ between two configs, and [`export_toml`] and
//! [`import`] move the settings between installs without the credentials.

use crate::config::{settings_table, Config};
use crate::config_lint::{self, ConfigWarning};
use crate::error::{CoreError, CoreResult};
use crate::pattern;
use crate::rules::RuleSet;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// How bad a [`Risk`] is
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    /// Can lose data
    Danger,
    /// Can cause loops, load or noise
    Warning,
}

/// A risky combination of settings
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Risk {
    /// Stable id, e.g. `delete_temp_folder`
    pub id: &'static str,
    /// How bad it is
    pub level: Level,
    /// Dotted keys of the settings involved
    pub keys: Vec<&'static str>,
    /// What can happen, in plain words
    pub message: String,
}

/// What the risk checks need to know about the machine the daemon runs on
#[derive(Debug, Clone, Default)]
pub struct Host {
    /// The daemon runs in a container
    pub in_container: bool,
    /// Device of the root filesystem (Unix), to tell a mounted volume from the container's
    /// own filesystem
    pub root_dev: Option<u64>,
    /// Folders whose contents may be cleared
    pub temp_dirs: Vec<PathBuf>,
}

impl Host {
    /// The host this process runs on
    pub fn current() -> Self {
        Self {
            in_container: crate::setup::in_container(),
            root_dev: device(Path::new("/")),
            temp_dirs: vec![
                std::env::temp_dir(),
                PathBuf::from("/tmp"),
                PathBuf::from("/var/tmp"),
            ],
        }
    }
}

/// The risky combinations in `config` on this host
pub fn risks(config: &Config) -> Vec<Risk> {
    risks_in(config, &Host::current())
}

/// The risky combinations in `config` on `host`, danger first, then by id
pub fn risks_in(config: &Config, host: &Host) -> Vec<Risk> {
    let download = &config.download;
    let target = download.target_directory.as_path();
    let mut found = Vec::new();
    let mut add = |id, level, keys: &[&'static str], message: String| {
        found.push(Risk {
            id,
            level,
            keys: keys.to_vec(),
            message,
        })
    };

    if download.delete_after_download {
        let keys = &[
            "download.delete_after_download",
            "download.target_directory",
        ];
        if let Some(temp) = host.temp_dirs.iter().find(|t| inside(target, t)) {
            add(
                "delete_temp_folder",
                Level::Danger,
                keys,
                format!(
                    "Files are deleted from Put.io after download, but the download folder {} is in {}, which can be cleared; the only copy could be lost.",
                    target.display(),
                    temp.display()
                ),
            );
        } else if host.in_container
            && host.root_dev.is_some()
            && device(&nearest_existing(target)) == host.root_dev
        {
            add(
                "delete_container_layer",
                Level::Danger,
                keys,
                format!(
                    "Files are deleted from Put.io after download, but the download folder {} is not on a mounted volume; the files are lost when the container is recreated.",
                    target.display()
                ),
            );
        }
    }

    let watch = &config.watch;
    if watch.enabled {
        if let Some(path) = watch.paths.iter().find(|p| inside(p, target)) {
            add(
                "watch_in_download",
                Level::Warning,
                &["watch.paths", "download.target_directory"],
                format!(
                    "The watch folder {} is inside the download folder, so downloaded .torrent files are uploaded again.",
                    path.display()
                ),
            );
        }
        if let Some(archive) = &watch.archive_directory {
            if watch.paths.iter().any(|p| inside(archive, p)) {
                add(
                    "archive_in_watch",
                    Level::Warning,
                    &["watch.archive_directory", "watch.paths"],
                    format!(
                        "The archive folder {} is inside a watch folder, so archived .torrent files are uploaded again.",
                        archive.display()
                    ),
                );
            }
        }
    }

    if config.cleanup.enabled && config.cleanup.min_age_hours == 0 {
        add(
            "cleanup_no_min_age",
            Level::Warning,
            &["cleanup.enabled", "cleanup.min_age_hours"],
            "The daily cleanup has no minimum age, so a folder a transfer has just created can be removed before its files arrive.".to_string(),
        );
    }

    if download.workers > MANY_WORKERS {
        add(
            "many_workers",
            Level::Warning,
            &["download.workers"],
            format!(
                "{} workers is more than {MANY_WORKERS}; Put.io may throttle or refuse requests.",
                download.workers
            ),
        );
    }

    // Bandwidth schedule mistakes (feature 032)
    for warning in crate::bandwidth::warnings(download) {
        add(warning.id, Level::Warning, &[warning.key], warning.message);
    }

    let level = config.logging.level.to_ascii_lowercase();
    if config.logging.file_enabled && (level == "trace" || level == "debug") {
        add(
            "verbose_log_file",
            Level::Warning,
            &["logging.level", "logging.file_enabled"],
            format!("The log file is on at level {level}, so it grows quickly."),
        );
    }

    let mut patterns: Vec<(&'static str, String, &str)> = Vec::new();
    for (key, what, text) in [
        (
            "download.whitelist_patterns",
            "Only download",
            &download.whitelist_patterns,
        ),
        (
            "download.blacklist_patterns",
            "Never download",
            &download.blacklist_patterns,
        ),
    ] {
        if let Some(text) = text {
            patterns.push((key, what.to_string(), text));
        }
    }
    for rule in &download.rules {
        for (what, text) in [
            ("Only download", &rule.include),
            ("Never download", &rule.exclude),
        ] {
            if let Some(text) = text {
                patterns.push((
                    "download.rules",
                    format!("Folder rule {}: {what}", rule.path),
                    text,
                ));
            }
        }
    }
    for (key, what, text) in patterns {
        for warning in pattern::check(text).unwrap_or_default() {
            add(
                "pattern_warning",
                Level::Warning,
                &[key],
                format!("{what} `{}`: {warning}.", text.trim()),
            );
        }
    }

    let rules = RuleSet::from_config(download);
    for rule in download
        .rules
        .iter()
        .filter(|r| r.delete_blocked == Some(true))
    {
        let include = rules.resolve(&rule.path).include.value;
        if !include.is_empty() {
            add(
                "delete_unmatched",
                Level::Warning,
                &["download.rules"],
                format!(
                    "Files under {} that don't match `{}` are deleted from Put.io at each sync.",
                    rule.path,
                    include.as_str()
                ),
            );
        }
    }

    // Event hooks (feature 027)
    for hook in &config.hooks {
        if let Some(url) = hook.url.as_deref().filter(|u| plain_http_off_lan(u)) {
            add(
                "hook_plain_http",
                Level::Warning,
                &["hooks"],
                format!(
                    "Hook {} posts events over plain http to {}, so they can be read on the way.",
                    hook.name,
                    crate::redact::url(url)
                ),
            );
        }
        if !hook.enabled {
            add(
                "hook_disabled",
                Level::Warning,
                &["hooks"],
                format!("Hook {} is turned off, so it gets no events.", hook.name),
            );
        }
        let program = hook.command.as_ref().and_then(|c| c.first()).map(Path::new);
        if let Some(dir) = program
            .and_then(Path::parent)
            .filter(|d| others_can_write(d))
        {
            add(
                "hook_shared_dir",
                Level::Warning,
                &["hooks"],
                format!(
                    "Hook {}'s program is in {}, which any user can write to, so anyone could change what the daemon runs.",
                    hook.name,
                    dir.display()
                ),
            );
        }
    }
    if config.notifications.enabled {
        add(
            "notifications_unused",
            Level::Warning,
            &["notifications.enabled"],
            "Notifications are on, but [notifications] sends nothing yet; use an event hook to be told about downloads.".to_string(),
        );
    }

    found.sort_by(|a, b| (a.level, a.id).cmp(&(b.level, b.id)));
    found
}

/// `url` is plain http to a host that is not loopback, a private address, `.local` or a
/// single label
fn plain_http_off_lan(url: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(url) else {
        return false;
    };
    if url.scheme() != "http" {
        return false;
    }
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let lan = match host.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(ip)) => ip.is_loopback() || ip.is_private() || ip.is_link_local(),
        Ok(std::net::IpAddr::V6(ip)) => {
            let first = ip.segments()[0];
            ip.is_loopback() || first & 0xfe00 == 0xfc00 || first & 0xffc0 == 0xfe80
        }
        Err(_) => {
            let name = host.trim_end_matches('.').to_ascii_lowercase();
            !name.contains('.') || name.ends_with(".local")
        }
    };
    !lan
}

/// Users other than the owner and group may write to `dir` (Unix mode `o+w`)
#[cfg(unix)]
fn others_can_write(dir: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(dir).is_ok_and(|m| m.permissions().mode() & 0o002 != 0)
}

#[cfg(not(unix))]
fn others_can_write(_dir: &Path) -> bool {
    false
}

/// More workers than this get a warning
const MANY_WORKERS: usize = 10;

/// `path` is `dir` or inside it, comparing as written and with symlinks resolved
fn inside(path: &Path, dir: &Path) -> bool {
    path.starts_with(dir) || resolve(path).starts_with(resolve(dir))
}

/// `path` with symlinks resolved as far as it exists
fn resolve(path: &Path) -> PathBuf {
    let existing = nearest_existing(path);
    match (
        std::fs::canonicalize(&existing),
        path.strip_prefix(&existing),
    ) {
        (Ok(real), Ok(rest)) => real.join(rest),
        _ => path.to_path_buf(),
    }
}

/// `path`, or its nearest parent that exists
fn nearest_existing(path: &Path) -> PathBuf {
    path.ancestors()
        .find(|p| p.exists())
        .unwrap_or(path)
        .to_path_buf()
}

#[cfg(unix)]
fn device(path: &Path) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path).ok().map(|m| m.dev())
}

#[cfg(not(unix))]
fn device(_path: &Path) -> Option<u64> {
    None
}

/// One setting that differs between two configs
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Change {
    /// Field name in a group, or dotted key
    pub key: String,
    /// The current value; none when unset
    pub before: Option<serde_json::Value>,
    /// The new value; none when removed
    pub after: Option<serde_json::Value>,
    /// Applies without a restart
    pub live: bool,
}

/// The settings that differ between `before` and `after`, by dotted key. Credentials and
/// `[arr]` are not compared. `live` is false; the caller knows what applies live.
pub fn diff(before: &Config, after: &Config) -> CoreResult<Vec<Change>> {
    let (before, after) = (
        flatten(&settings_table(&masked(before))?),
        flatten(&settings_table(&masked(after))?),
    );
    let keys: std::collections::BTreeSet<&String> = before.keys().chain(after.keys()).collect();
    let json = |v: Option<&toml::Value>| v.and_then(|v| serde_json::to_value(v).ok());
    Ok(keys
        .into_iter()
        .filter(|key| before.get(*key) != after.get(*key))
        .map(|key| Change {
            key: key.clone(),
            before: json(before.get(key)),
            after: json(after.get(key)),
            live: false,
        })
        .collect())
}

/// Tables flattened to dotted keys; arrays are compared whole
fn flatten(table: &toml::Table) -> BTreeMap<String, toml::Value> {
    fn walk(prefix: &str, table: &toml::Table, out: &mut BTreeMap<String, toml::Value>) {
        for (key, value) in table {
            let path = if prefix.is_empty() {
                key.clone()
            } else {
                format!("{prefix}.{key}")
            };
            match value {
                toml::Value::Table(inner) => walk(&path, inner, out),
                other => {
                    out.insert(path, other.clone());
                }
            }
        }
    }
    let mut out = BTreeMap::new();
    walk("", table, &mut out);
    out
}

const EXPORT_HEADER: &str = "# putio-sync settings export\n\
# Credentials ([auth], api.auth_token) and the Radarr/Sonarr section ([arr]) are left out.\n\
# Hook header and env values read \"********\"; an import keeps the stored ones.\n\
# Import it from Settings > Backup and restore.\n\n";

/// `config` with every hook header and env value replaced by
/// [`MASK`](crate::hooks::MASK) (feature 027)
fn masked(config: &Config) -> std::borrow::Cow<'_, Config> {
    if config.hooks.is_empty() {
        return std::borrow::Cow::Borrowed(config);
    }
    let mut config = config.clone();
    config.hooks = crate::hooks::mask(&config.hooks);
    std::borrow::Cow::Owned(config)
}

/// The settings as TOML, without `[auth]`, `api.auth_token` and `[arr]`, and with hook
/// header and env values masked
pub fn export_toml(config: &Config) -> CoreResult<String> {
    let body = toml::to_string_pretty(&settings_table(&masked(config))?)
        .map_err(|e| CoreError::ConfigError(format!("Failed to serialize config: {e}")))?;
    Ok(format!("{EXPORT_HEADER}{body}"))
}

/// An imported settings file merged over the current config
#[derive(Debug, Clone)]
pub struct Import {
    /// The current config with the file's settings applied; credentials and `[arr]` are
    /// the current ones
    pub config: Config,
    /// Parts of the file that were left out (`auth`, `arr`, `api.auth_token`)
    pub ignored: Vec<String>,
    /// Unknown or risky keys, as the doctor reports them
    pub unknown: Vec<ConfigWarning>,
}

/// Merge the settings in `text` over `current`. Settings the file leaves out stay as they
/// are; its credentials and `[arr]` are ignored. Errors name the line, never quote it.
pub fn import(current: &Config, text: &str) -> CoreResult<Import> {
    let not_toml = |e: toml::de::Error| {
        let at = e
            .span()
            .map(|s| format!(" (line {})", line_of(text, s.start)))
            .unwrap_or_default();
        CoreError::ConfigError(format!("The file is not valid TOML{at}: {}", e.message()))
    };
    let mut file: toml::Table = text.parse().map_err(not_toml)?;

    let mut ignored = Vec::new();
    for key in ["auth", "arr"] {
        if file.remove(key).is_some() {
            ignored.push(key.to_string());
        }
    }
    if let Some(toml::Value::Table(api)) = file.get_mut("api") {
        if api.remove("auth_token").is_some() {
            ignored.push("api.auth_token".to_string());
        }
    }

    let mut merged = settings_table(current)?;
    merge(&mut merged, file);
    // The current settings have no unknown keys, so these are the file's
    let text = toml::to_string(&merged)
        .map_err(|e| CoreError::ConfigError(format!("Failed to read the file: {e}")))?;
    let unknown = config_lint::lint(&text).map_err(|e| {
        CoreError::ConfigError(format!("The file is not a valid config: {}", e.message()))
    })?;
    let mut config: Config =
        toml::Value::Table(merged)
            .try_into()
            .map_err(|e: toml::de::Error| {
                CoreError::ConfigError(format!("The file is not a valid config: {}", e.message()))
            })?;
    config.auth = current.auth.clone();
    config.arr = current.arr.clone();
    config.api.auth_token = current.api.auth_token.clone();
    // An exported file's masked hook values are the stored ones; a mask for a hook or
    // key this config lacks is refused
    config.hooks = crate::hooks::unmask(&config.hooks, &current.hooks)?;
    config.expand_paths()?;
    Ok(Import {
        config,
        ignored,
        unknown,
    })
}

/// Put every key of `from` into `into`, table by table
fn merge(into: &mut toml::Table, from: toml::Table) {
    for (key, value) in from {
        match (into.get_mut(&key), value) {
            (Some(toml::Value::Table(old)), toml::Value::Table(new)) => merge(old, new),
            (_, value) => {
                into.insert(key, value);
            }
        }
    }
}

/// Line number (1-based) of byte `offset` in `text`
fn line_of(text: &str, offset: usize) -> usize {
    text.as_bytes()[..offset.min(text.len())]
        .iter()
        .filter(|b| **b == b'\n')
        .count()
        + 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ArrKind, ArrServer, AuthConfig};
    use crate::rules::FolderRule;

    /// A host with no container and `temp` as its only temporary folder
    fn host(temp: &Path) -> Host {
        Host {
            in_container: false,
            root_dev: None,
            temp_dirs: vec![temp.to_path_buf()],
        }
    }

    fn ids(config: &Config, host: &Host) -> Vec<&'static str> {
        risks_in(config, host).into_iter().map(|r| r.id).collect()
    }

    #[test]
    fn pattern_warnings_name_the_setting() {
        let mut config = Config::default();
        config.download.target_directory = PathBuf::from("/srv/dl");
        config.download.whitelist_patterns = Some("*.mkv OR".into());
        config.download.rules = vec![FolderRule {
            path: "Movies".into(),
            exclude: Some("*[Ss]ample*".into()),
            ..FolderRule::default()
        }];
        let found = risks_in(&config, &host(Path::new("/scratch")));
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(found.iter().all(|r| r.id == "pattern_warning"));
        assert_eq!(found[0].keys, ["download.whitelist_patterns"]);
        assert!(found[0].message.contains("Only download"));
        assert_eq!(found[1].keys, ["download.rules"]);
        assert!(found[1]
            .message
            .contains("Folder rule Movies: Never download"));
        assert!(found[1].message.contains("brackets are literal"));
    }

    #[test]
    fn delete_blocked_with_an_only_download_pattern() {
        let mut config = Config::default();
        config.download.target_directory = PathBuf::from("/srv/dl");
        config.download.rules = vec![FolderRule {
            path: "Movies".into(),
            delete_blocked: Some(true),
            ..FolderRule::default()
        }];
        let h = host(Path::new("/scratch"));
        assert!(ids(&config, &h).is_empty(), "no pattern");
        config.download.whitelist_patterns = Some("*.mkv".into());
        let found = risks_in(&config, &h);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, "delete_unmatched");
        assert_eq!(
            found[0].message,
            "Files under Movies that don't match `*.mkv` are deleted from Put.io at each sync."
        );
        config.download.rules[0].include = Some(String::new());
        assert!(ids(&config, &h).is_empty(), "the rule clears it");
    }

    #[test]
    fn default_config_has_no_risks() {
        let mut config = Config::default();
        config.download.target_directory = PathBuf::from("/srv/downloads");
        assert!(ids(&config, &host(Path::new("/scratch"))).is_empty());
    }

    #[test]
    fn delete_into_temp_folder_is_danger() {
        let temp = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.download.target_directory = temp.path().join("dl/not-yet");
        assert!(ids(&config, &host(temp.path())).is_empty());
        config.download.delete_after_download = true;
        let found = risks_in(&config, &host(temp.path()));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, "delete_temp_folder");
        assert_eq!(found[0].level, Level::Danger);
        assert!(found[0].keys.contains(&"download.delete_after_download"));
    }

    #[test]
    fn delete_on_container_layer_is_danger_only_in_a_container() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.download.target_directory = dir.path().join("dl");
        config.download.delete_after_download = true;
        let mut h = host(Path::new("/nowhere"));
        h.root_dev = device(dir.path());
        assert!(ids(&config, &h).is_empty());
        h.in_container = true;
        assert_eq!(ids(&config, &h), ["delete_container_layer"]);
        // A mounted volume is a different device
        h.root_dev = h.root_dev.map(|d| d + 1);
        assert!(ids(&config, &h).is_empty());
    }

    #[test]
    fn watch_folder_inside_download_folder() {
        let mut config = Config::default();
        config.download.target_directory = PathBuf::from("/srv/dl");
        config.watch.paths = vec![PathBuf::from("/srv/dl/torrents")];
        let h = host(Path::new("/scratch"));
        assert!(ids(&config, &h).is_empty(), "watcher off");
        config.watch.enabled = true;
        assert_eq!(ids(&config, &h), ["watch_in_download"]);
        config.watch.paths = vec![PathBuf::from("/srv/dl-torrents")];
        assert!(ids(&config, &h).is_empty(), "a sibling, not inside");
    }

    #[test]
    fn archive_inside_watch_folder() {
        let mut config = Config::default();
        config.download.target_directory = PathBuf::from("/srv/dl");
        config.watch.enabled = true;
        config.watch.paths = vec![PathBuf::from("/srv/watch")];
        config.watch.archive_directory = Some(PathBuf::from("/srv/watch/done"));
        let h = host(Path::new("/scratch"));
        assert_eq!(ids(&config, &h), ["archive_in_watch"]);
        config.watch.archive_directory = Some(PathBuf::from("/srv/done"));
        assert!(ids(&config, &h).is_empty());
    }

    #[test]
    fn cleanup_workers_and_log_level() {
        let mut config = Config::default();
        config.download.target_directory = PathBuf::from("/srv/dl");
        let h = host(Path::new("/scratch"));
        config.cleanup.enabled = true;
        config.cleanup.min_age_hours = 0;
        config.download.workers = 11;
        config.logging.level = "DEBUG".to_string();
        assert_eq!(
            ids(&config, &h),
            ["cleanup_no_min_age", "many_workers", "verbose_log_file"]
        );
        config.cleanup.min_age_hours = 1;
        config.download.workers = 10;
        config.logging.file_enabled = false;
        assert!(ids(&config, &h).is_empty());
    }

    #[test]
    fn bandwidth_schedule_mistakes() {
        use crate::bandwidth::{Limit, ScheduleEntry};
        let mut config = Config::default();
        config.download.target_directory = PathBuf::from("/srv/dl");
        let h = host(Path::new("/scratch"));
        let entry = |start: &str, end: &str, limit| ScheduleEntry {
            days: "daily".into(),
            start: start.into(),
            end: end.into(),
            limit,
        };
        config.download.bandwidth_schedule = vec![
            entry("01:00", "07:00", Limit::Unlimited),
            entry("06:00", "08:00", Limit::Rate(1000)),
        ];
        assert_eq!(ids(&config, &h), ["bandwidth_low", "bandwidth_overlap"]);
        config.download.bandwidth_schedule = vec![entry("00:00", "24:00", Limit::Paused)];
        assert_eq!(ids(&config, &h), ["bandwidth_paused_week"]);
        config.download.bandwidth_schedule.clear();
        assert!(ids(&config, &h).is_empty());
    }

    #[test]
    fn danger_sorts_first() {
        let temp = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.download.target_directory = temp.path().join("dl");
        config.download.delete_after_download = true;
        config.download.workers = 20;
        assert_eq!(
            ids(&config, &host(temp.path())),
            ["delete_temp_folder", "many_workers"]
        );
    }

    /// A loaded config (paths expanded) with every kind of secret set
    fn with_secrets() -> Config {
        let mut config = Config::default();
        config.expand_paths().unwrap();
        config.download.target_directory = PathBuf::from("/srv/dl");
        config.auth = Some(AuthConfig::with_token("putio-secret-token".to_string()));
        config.api.auth_token = Some("api-secret-token".to_string());
        config.arr.password = Some("arr-secret-password".to_string());
        config.arr.servers.push(ArrServer {
            kind: ArrKind::Radarr,
            name: None,
            url: "http://radarr:7878".to_string(),
            api_key: Some("arr-secret-key".to_string()),
            category: None,
        });
        config.hooks.push(crate::hooks::HookConfig {
            name: "ha".to_string(),
            events: vec!["*".to_string()],
            url: Some("https://ha.lan/api/webhook".to_string()),
            headers: [(
                "Authorization".to_string(),
                "hook-secret-header".to_string(),
            )]
            .into(),
            ..Default::default()
        });
        config.hooks.push(crate::hooks::HookConfig {
            name: "script".to_string(),
            events: vec!["*".to_string()],
            command: Some(vec!["/bin/true".to_string()]),
            env: [("API_KEY".to_string(), "hook-secret-env".to_string())].into(),
            ..Default::default()
        });
        config
    }

    fn assert_no_secrets(text: &str) {
        for secret in [
            "putio-secret-token",
            "api-secret-token",
            "arr-secret-password",
            "arr-secret-key",
            "hook-secret-header",
            "hook-secret-env",
        ] {
            assert!(!text.contains(secret), "{secret} leaked");
        }
    }

    #[test]
    fn export_leaves_out_credentials_and_arr() {
        let text = export_toml(&with_secrets()).unwrap();
        assert_no_secrets(&text);
        let table: toml::Table = text.parse().unwrap();
        assert!(table.get("auth").is_none());
        assert!(table.get("arr").is_none());
        assert!(table["api"].get("auth_token").is_none());
        assert!(table.contains_key("download"));
    }

    #[test]
    fn hook_risks() {
        let temp = tempfile::tempdir().unwrap();
        let shared = temp.path().join("shared");
        std::fs::create_dir(&shared).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&shared, std::fs::Permissions::from_mode(0o777)).unwrap();
        }
        let hook = |name: &str| crate::hooks::HookConfig {
            name: name.to_string(),
            events: vec!["*".to_string()],
            ..Default::default()
        };
        let mut config = Config::default();
        config.download.target_directory = temp.path().join("dl");
        for (name, url) in [
            ("lan", "http://192.168.1.5/hook"),
            ("local", "http://ha.local:8123/api"),
            ("label", "http://homeassistant:8123/api"),
            ("tls", "https://example.com/hook"),
        ] {
            config.hooks.push(crate::hooks::HookConfig {
                url: Some(url.to_string()),
                ..hook(name)
            });
        }
        assert!(ids(&config, &host(temp.path())).is_empty());

        config.hooks.push(crate::hooks::HookConfig {
            url: Some("http://example.com/hook?token=t".to_string()),
            enabled: false,
            ..hook("far")
        });
        config.hooks.push(crate::hooks::HookConfig {
            command: Some(vec![shared.join("run.sh").display().to_string()]),
            ..hook("shared")
        });
        config.notifications.enabled = true;
        let risks = risks_in(&config, &host(temp.path()));
        let ids: Vec<_> = risks.iter().map(|r| r.id).collect();
        let mut expected = vec!["hook_disabled", "hook_plain_http", "notifications_unused"];
        if cfg!(unix) {
            expected.insert(2, "hook_shared_dir");
        }
        assert_eq!(ids, expected);
        let plain = risks.iter().find(|r| r.id == "hook_plain_http").unwrap();
        assert!(plain.message.contains("far") && !plain.message.contains("token"));
    }

    #[test]
    fn hook_values_are_masked_and_an_import_keeps_the_stored_ones() {
        let current = with_secrets();
        let text = export_toml(&current).unwrap();
        assert!(text.contains(crate::hooks::MASK));
        let imported = import(&current, &text).unwrap();
        assert_eq!(imported.config.hooks, current.hooks);

        // A mask for a hook this config lacks can't be resolved
        let renamed = text.replace("name = \"ha\"", "name = \"other\"");
        let err = import(&current, &renamed).unwrap_err().to_string();
        assert!(
            err.contains("hooks \"other\".headers.Authorization: still masked"),
            "{err}"
        );

        // A diff never shows the values
        let mut changed = current.clone();
        changed.hooks[0]
            .headers
            .insert("Authorization".into(), "new-secret".into());
        changed.hooks[0].timeout_secs = 5;
        let changes = serde_json::to_string(&diff(&current, &changed).unwrap()).unwrap();
        assert!(changes.contains("hooks"));
        assert_no_secrets(&changes);
        assert!(!changes.contains("new-secret"));
    }

    #[test]
    fn export_then_import_changes_nothing() {
        let current = with_secrets();
        let text = export_toml(&current).unwrap();
        let imported = import(&current, &text).unwrap();
        let changes = diff(&current, &imported.config).unwrap();
        assert!(changes.is_empty(), "{changes:?}");
        assert!(imported.ignored.is_empty());
        assert!(imported.unknown.is_empty(), "{:?}", imported.unknown);
    }

    #[test]
    fn partial_import_changes_only_its_keys_and_keeps_credentials() {
        let current = with_secrets();
        let text = "[download]\nworkers = 7\n\n[auth]\noauth_token = \"other\"\n\n[api]\nauth_token = \"other\"\n\n[arr]\npassword = \"other\"\n";
        let imported = import(&current, text).unwrap();
        let changes = diff(&current, &imported.config).unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].key, "download.workers");
        assert_eq!(changes[0].before, Some(serde_json::json!(2)));
        assert_eq!(changes[0].after, Some(serde_json::json!(7)));
        assert_eq!(imported.ignored, ["auth", "arr", "api.auth_token"]);
        assert_eq!(
            imported.config.api.auth_token.as_deref(),
            Some("api-secret-token")
        );
        assert_eq!(
            imported.config.arr.password.as_deref(),
            Some("arr-secret-password")
        );
        assert_eq!(
            crate::setup::putio_token(&imported.config),
            Some("putio-secret-token")
        );
    }

    #[test]
    fn import_reports_unknown_keys() {
        let imported = import(&Config::default(), "[download]\nworkerz = 3\n").unwrap();
        assert_eq!(imported.unknown.len(), 1);
        assert_eq!(imported.unknown[0].key, "download.workerz");
    }

    #[test]
    fn import_errors_name_the_line_not_its_text() {
        let err = import(&Config::default(), "[download]\nworkers = \"secret-ish\n")
            .unwrap_err()
            .to_string();
        assert!(err.contains("line 2"), "{err}");
        assert!(!err.contains("secret-ish"), "{err}");
        let err = import(&Config::default(), "[download]\nworkers = \"many\"\n")
            .unwrap_err()
            .to_string();
        assert!(err.contains("not a valid config"), "{err}");
    }
}
