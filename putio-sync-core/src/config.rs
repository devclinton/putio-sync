//! Configuration management for putio-sync
//!
//! This module handles loading, saving, and managing configuration files
//! in TOML format.

use crate::error::{CoreError, CoreResult};
use crate::path_utils::expand_path;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::logging::LevelSource;
use putio_sync_telemetry::server::MetricsSettings;
use putio_sync_telemetry::Detail;

/// Default value for delete_empty_folders
fn default_delete_empty_folders() -> bool {
    true
}

fn default_skip_existing() -> bool {
    true
}

/// Extensions that are never downloaded unless `blocked_extensions` overrides them.
/// Fake releases on the NAS were mostly `.exe` files (133 of 244 downloads).
pub const DEFAULT_BLOCKED_EXTENSIONS: &[&str] = &[
    "exe", "msi", "bat", "cmd", "com", "scr", "pif", "vbs", "js", "jar", "ps1", "lnk", "apk",
    "dmg", "pkg",
];

/// Default value for blocked_extensions
pub fn default_blocked_extensions() -> Vec<String> {
    DEFAULT_BLOCKED_EXTENSIONS
        .iter()
        .map(|e| e.to_string())
        .collect()
}

/// Main configuration structure
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Config {
    /// Authentication configuration
    pub auth: Option<AuthConfig>,
    /// Sync configuration
    #[serde(default)]
    pub sync: SyncConfig,
    /// Download configuration
    #[serde(default)]
    pub download: DownloadConfig,
    /// Watch configuration
    #[serde(default)]
    pub watch: WatchConfig,
    /// Notification configuration
    #[serde(default)]
    pub notifications: NotificationConfig,
    /// API/Daemon mode configuration
    #[serde(default)]
    pub api: ApiConfig,
    /// Logging configuration
    #[serde(default)]
    pub logging: crate::logging::LoggingConfig,
    /// Prometheus metrics configuration
    #[serde(default)]
    pub metrics: MetricsConfig,
    /// Put.io empty-folder cleanup
    #[serde(default)]
    pub cleanup: CleanupConfig,
    /// Put.io transfer monitoring (stall detection)
    #[serde(default)]
    pub transfers: TransfersConfig,
    /// Radarr/Sonarr download client (Transmission RPC)
    #[serde(default)]
    pub arr: ArrConfig,
    /// Scheduled doctor runs (feature 010)
    #[serde(default)]
    pub doctor: DoctorConfig,
}

/// `[doctor]`: how often the daemon runs the doctor's checks (feature 010). The CLI and
/// the web UI can run them at any time, whatever this says.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(default)]
pub struct DoctorConfig {
    /// Run the checks on a schedule in the daemon
    pub enabled: bool,
    /// Minutes between scheduled runs; values below 5 count as 5
    pub interval_minutes: u64,
}

impl Default for DoctorConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            interval_minutes: 60,
        }
    }
}

impl DoctorConfig {
    /// Time between scheduled runs
    pub fn interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.interval_minutes.max(5) * 60)
    }
}

/// Default `[arr] username`
pub const DEFAULT_ARR_USERNAME: &str = "putio-sync";

/// `[arr]`: putio-sync answers Radarr's and Sonarr's Transmission client (feature 011).
/// The password and API keys may come from the environment instead, see
/// [`Config::resolve_arr`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(default)]
pub struct ArrConfig {
    /// Answer the Transmission RPC at `/transmission/rpc`
    pub enabled: bool,
    /// Username the *arr apps send
    pub username: String,
    /// Password the *arr apps send. `PUTIO_SYNC_ARR_PASSWORD` overrides it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    /// *arr apps whose API putio-sync calls to blocklist failed releases
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub servers: Vec<ArrServer>,
}

impl Default for ArrConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            username: DEFAULT_ARR_USERNAME.to_string(),
            password: None,
            servers: Vec::new(),
        }
    }
}

impl std::fmt::Debug for ArrConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ArrConfig")
            .field("enabled", &self.enabled)
            .field("username", &self.username)
            .field("password", &redacted(&self.password))
            .field("servers", &self.servers)
            .finish()
    }
}

/// Which *arr app a `[[arr.servers]]` entry is
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ArrKind {
    /// Radarr (movies)
    Radarr,
    /// Sonarr (series)
    Sonarr,
}

impl ArrKind {
    /// `radarr` or `sonarr`
    pub fn as_str(self) -> &'static str {
        match self {
            ArrKind::Radarr => "radarr",
            ArrKind::Sonarr => "sonarr",
        }
    }

    /// The `appName` its `/api/v3/system/status` reports
    pub fn app_name(self) -> &'static str {
        match self {
            ArrKind::Radarr => "Radarr",
            ArrKind::Sonarr => "Sonarr",
        }
    }
}

/// `[[arr.servers]]`: one *arr app and the category it sends
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct ArrServer {
    /// `radarr` or `sonarr`
    pub kind: ArrKind,
    /// Base URL, e.g. `http://192.168.1.34:8989`
    pub url: String,
    /// API key. `PUTIO_SYNC_RADARR_API_KEY` / `PUTIO_SYNC_SONARR_API_KEY` override it
    /// for the first server of that kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    /// The category set on the *arr's Transmission client (the folder under the
    /// download folder)
    pub category: String,
}

impl std::fmt::Debug for ArrServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ArrServer")
            .field("kind", &self.kind)
            .field("url", &self.url)
            .field("api_key", &redacted(&self.api_key))
            .field("category", &self.category)
            .finish()
    }
}

/// `Debug` of a secret: `<set>` or `<unset>`
struct Redacted(bool);

impl std::fmt::Debug for Redacted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.0 { "<set>" } else { "<unset>" })
    }
}

fn redacted(secret: &Option<String>) -> Redacted {
    Redacted(secret.is_some())
}

/// Normalise a category: no leading or trailing `/`. `None` when it has an empty or
/// `.`/`..` segment or a backslash.
pub fn normalize_arr_category(category: &str) -> Option<String> {
    let trimmed = category.trim_matches('/');
    if trimmed.is_empty() {
        return Some(String::new());
    }
    let ok = !trimmed.contains('\\')
        && trimmed
            .split('/')
            .all(|seg| !seg.is_empty() && seg != "." && seg != "..");
    ok.then(|| trimmed.to_string())
}

impl ArrConfig {
    /// Check each server's URL and category, and that categories are unique
    pub fn validate(&self) -> CoreResult<()> {
        if self.username.is_empty() || self.username.contains(':') {
            return Err(CoreError::ConfigError(
                "invalid arr username: expected a non-empty value without ':'".to_string(),
            ));
        }
        let mut seen = std::collections::HashSet::new();
        for server in &self.servers {
            let url = server.url.trim_end_matches('/');
            let rest = url
                .strip_prefix("http://")
                .or_else(|| url.strip_prefix("https://"));
            if rest.is_none_or(|r| r.is_empty() || r.chars().any(char::is_whitespace)) {
                return Err(invalid("arr server url", &server.url, "an http(s):// URL"));
            }
            let category = normalize_arr_category(&server.category).ok_or_else(|| {
                invalid(
                    "arr server category",
                    &server.category,
                    "a relative folder without empty, . or .. segments",
                )
            })?;
            if !seen.insert(category.clone()) {
                return Err(CoreError::ConfigError(format!(
                    "arr server category '{}' is used twice",
                    category
                )));
            }
        }
        Ok(())
    }
}

/// `[arr]` after env overrides and validation
#[derive(Clone, PartialEq, Eq)]
pub struct ArrSettings {
    /// Answer the Transmission RPC
    pub enabled: bool,
    /// Username the *arr apps send
    pub username: String,
    /// Password the *arr apps send; the RPC answers 503 without one
    pub password: Option<String>,
    /// Servers with normalised categories and URLs without a trailing `/`
    pub servers: Vec<ArrServer>,
}

impl std::fmt::Debug for ArrSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ArrSettings")
            .field("enabled", &self.enabled)
            .field("username", &self.username)
            .field("password", &redacted(&self.password))
            .field("servers", &self.servers)
            .finish()
    }
}

impl ArrSettings {
    /// The server whose category is `category` (normalised)
    pub fn server_for(&self, category: &str) -> Option<&ArrServer> {
        let category = normalize_arr_category(category)?;
        self.servers.iter().find(|s| s.category == category)
    }
}

/// `[transfers]`: when a Put.io transfer counts as stalled (feature 007)
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(default)]
pub struct TransfersConfig {
    /// Hours without new bytes before a downloading transfer is stalled
    pub stall_threshold_hours: u64,
    /// Seconds between checks of the Put.io transfer list
    pub stall_check_interval_secs: u64,
}

impl Default for TransfersConfig {
    fn default() -> Self {
        Self {
            stall_threshold_hours: 6,
            stall_check_interval_secs: 600,
        }
    }
}

/// Authentication method
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
#[serde(rename_all = "PascalCase")]
pub enum AuthMethod {
    /// OAuth token authentication (Bearer token)
    #[default]
    Token,
    /// App-specific password authentication (Basic Auth)
    AppPassword,
}

/// Authentication configuration
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AuthConfig {
    /// Authentication method to use
    #[serde(default)]
    pub auth_method: AuthMethod,

    /// OAuth token (used when auth_method is Token)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,

    /// Put.io username/email (used when auth_method is AppPassword)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,

    /// App-specific password (used when auth_method is AppPassword)
    /// Create at: https://app.put.io/account/app-specific-passwords
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
}

impl AuthConfig {
    /// Create a new AuthConfig with OAuth token
    pub fn with_token(token: String) -> Self {
        Self {
            auth_method: AuthMethod::Token,
            token: Some(token),
            username: None,
            password: None,
        }
    }

    /// Create a new AuthConfig with app-specific password
    pub fn with_app_password(username: String, password: String) -> Self {
        Self {
            auth_method: AuthMethod::AppPassword,
            token: None,
            username: Some(username),
            password: Some(password),
        }
    }

    /// Validate the authentication configuration
    pub fn validate(&self) -> CoreResult<()> {
        match self.auth_method {
            AuthMethod::Token => {
                if self.token.is_none() || self.token.as_ref().unwrap().is_empty() {
                    return Err(CoreError::ConfigError(
                        "Token is required for Token authentication".to_string(),
                    ));
                }
            }
            AuthMethod::AppPassword => {
                if self.username.is_none() || self.username.as_ref().unwrap().is_empty() {
                    return Err(CoreError::ConfigError(
                        "Username is required for AppPassword authentication".to_string(),
                    ));
                }
                if self.password.is_none() || self.password.as_ref().unwrap().is_empty() {
                    return Err(CoreError::ConfigError(
                        "Password is required for AppPassword authentication".to_string(),
                    ));
                }
            }
        }
        Ok(())
    }
}

/// Sync configuration
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SyncConfig {
    /// Local path where files will be synced
    pub local_path: PathBuf,
    /// Remote parent folder ID (None or Some(0) for root)
    pub remote_parent_id: Option<i64>,
    /// Delete files from Put.io after successful download
    pub delete_after_download: bool,
}

/// Progress bar display style
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ProgressStyle {
    /// One bar per file + overall progress bar
    #[default]
    PerFile,
    /// Overall progress only
    Overall,
    /// Individual file progress only
    Individual,
    /// Rotate between files and overall
    Rotating,
    /// No progress bars
    None,
}

/// Sort order for downloads
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum SortOrder {
    /// Sort by date (default)
    #[default]
    ByDate,
    /// Sort by filename
    ByName,
    /// Sort by file extension
    ByExtension,
    /// Sort by file size
    BySize,
}

/// Download configuration
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DownloadConfig {
    /// Number of concurrent download workers (default: 2)
    pub workers: usize,
    /// Target directory for downloads
    pub target_directory: PathBuf,
    /// Maximum number of concurrent downloads (alias for workers, for backwards compatibility)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_concurrent: Option<usize>,
    /// Buffer size in MB for streaming downloads (default: 16)
    pub buffer_size_mb: usize,
    /// Chunk size in MB for chunked downloads (None to disable - Put.io doesn't support range requests)
    pub chunk_size_mb: Option<usize>,
    /// Enable resume support for interrupted downloads
    pub resume_enabled: bool,
    /// Skip files that already exist in the target directory (default true; false
    /// re-downloads and overwrites them)
    #[serde(default = "default_skip_existing")]
    pub skip_existing: bool,
    /// Delete files from Put.io after successful download
    pub delete_after_download: bool,
    /// Progress bar display style
    #[serde(default)]
    pub progress_style: ProgressStyle,
    /// Sort order for downloads
    #[serde(default)]
    pub sort_order: SortOrder,
    /// Reverse the sort order
    pub reverse_sort: bool,
    /// Glob patterns to whitelist files (e.g., "*.mkv")
    pub whitelist_patterns: Option<String>,
    /// Glob patterns to blacklist files (e.g., "*.exe")
    pub blacklist_patterns: Option<String>,
    /// Bandwidth limit in bytes per second (None for unlimited)
    /// Example: Some(1048576) for 1 MB/s limit
    pub bandwidth_limit_bytes_per_sec: Option<u64>,
    /// Maximum number of retry attempts for failed downloads (default: 3)
    pub max_retries: u32,
    /// Initial retry delay in seconds (doubles with each retry)
    pub retry_initial_delay_secs: u64,
    /// Delete empty folders after downloading all files from them (default: true)
    #[serde(default = "default_delete_empty_folders")]
    pub delete_empty_folders: bool,
    /// File extensions (without the dot, case-insensitive) that are never downloaded.
    /// A missing key uses the default list; `[]` disables the guard.
    #[serde(default = "default_blocked_extensions")]
    pub blocked_extensions: Vec<String>,
}

/// Watch configuration
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WatchConfig {
    /// Enable watch mode
    #[serde(default)]
    pub enabled: bool,
    /// Paths to watch for changes
    pub paths: Vec<PathBuf>,
    /// How often to refresh in seconds (used when file system events aren't available)
    pub refresh_cycle: i8,
    /// Parent folder ID for uploaded torrents (None or 0 for root)
    #[serde(default)]
    pub parent_folder_id: Option<i64>,
    /// Archive directory for processed torrents (moves files here after upload)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archive_directory: Option<PathBuf>,
    /// Delete torrent files after successful upload
    #[serde(default)]
    pub delete_after_upload: bool,
}

/// Notification backend type
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum NotificationBackend {
    /// No notifications
    #[default]
    None,
    /// Webhook notifications (HTTP POST)
    Webhook,
    /// Email notifications (future)
    Email,
}

/// Notification configuration
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NotificationConfig {
    /// Enable notifications
    pub enabled: bool,
    /// Notification backend to use
    pub backend: NotificationBackend,
    /// Webhook URL (used when backend is Webhook)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub webhook_url: Option<String>,
    /// Email address (used when backend is Email, future)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}

/// API/Daemon mode configuration
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct ApiConfig {
    /// Enable daemon to communicate with API server for downloads
    /// When true, the daemon will send download requests to an API server
    /// instead of downloading files directly
    pub enabled: bool,
    /// API server URL (e.g., "http://localhost:15432")
    /// Required when enabled is true
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// API authentication token for securing REST API endpoints
    /// When set, all API requests must include this token in the Authorization header
    /// Format: "Bearer <token>"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_token: Option<String>,
    /// `daemon` opens the API and web UI listener. A bool or `true`/`false`/`1`/`0`,
    /// checked by [`Config::resolve_api`]. Default `true`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub listen: Option<Flag>,
    /// Listener address for `daemon`, `ip:port`. Default `0.0.0.0:15432`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bind: Option<String>,
}

/// A boolean setting written either as a TOML bool or as a string, so a bad string
/// gets the same error from the file as from the environment.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(untagged)]
pub enum Flag {
    /// `listen = true`
    Bool(bool),
    /// `listen = "1"`
    Text(String),
}

/// Default `[api] bind`
pub const DEFAULT_API_BIND: &str = "0.0.0.0:15432";

/// `[api]` listener settings after env overrides and validation
#[derive(Clone, PartialEq, Eq)]
pub struct ApiSettings {
    /// Open the listener (`daemon` only)
    pub listen: bool,
    /// Listener address (`daemon` only)
    pub bind: std::net::SocketAddr,
    /// Bearer token every protected route requires
    pub token: Option<String>,
}

impl std::fmt::Debug for ApiSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApiSettings")
            .field("listen", &self.listen)
            .field("bind", &self.bind)
            .field(
                "token",
                &format_args!(
                    "{}",
                    if self.token.is_some() {
                        "<set>"
                    } else {
                        "<unset>"
                    }
                ),
            )
            .finish()
    }
}

impl Default for SyncConfig {
    fn default() -> Self {
        SyncConfig {
            local_path: PathBuf::from("./putio-downloads"),
            remote_parent_id: Some(0), // Root folder
            delete_after_download: false,
        }
    }
}

impl Default for DownloadConfig {
    fn default() -> Self {
        DownloadConfig {
            workers: 2,
            target_directory: PathBuf::from("./putio-downloads"),
            max_concurrent: None,
            buffer_size_mb: 16,
            chunk_size_mb: None, // Disabled - Put.io doesn't support range requests
            resume_enabled: true,
            skip_existing: true,
            delete_after_download: false,
            progress_style: ProgressStyle::PerFile,
            sort_order: SortOrder::ByDate,
            reverse_sort: false,
            whitelist_patterns: None,
            blacklist_patterns: None,
            bandwidth_limit_bytes_per_sec: None, // Unlimited by default
            max_retries: 3,                      // 3 retries by default
            retry_initial_delay_secs: 1,         // Start with 1 second delay
            delete_empty_folders: true,          // Delete empty folders by default
            blocked_extensions: default_blocked_extensions(),
        }
    }
}

impl WatchConfig {
    /// Validate the watch configuration
    pub fn validate(&self) -> CoreResult<()> {
        // Cannot have both archive and delete enabled
        if self.archive_directory.is_some() && self.delete_after_upload {
            return Err(CoreError::ConfigError(
                "Cannot have both archive_directory and delete_after_upload enabled. Choose one or neither.".to_string(),
            ));
        }

        // If archive directory is set, validate it can be created
        if let Some(ref archive_dir) = self.archive_directory {
            if let Some(parent) = archive_dir.parent() {
                if !parent.exists() {
                    return Err(CoreError::ConfigError(format!(
                        "Archive directory parent does not exist: {}",
                        parent.display()
                    )));
                }
            }
        }

        Ok(())
    }
}

impl Default for WatchConfig {
    fn default() -> Self {
        WatchConfig {
            enabled: false,
            paths: vec![],
            refresh_cycle: 60,         // 60 seconds
            parent_folder_id: Some(0), // Root folder
            archive_directory: None,
            delete_after_upload: true,
        }
    }
}

impl Default for NotificationConfig {
    fn default() -> Self {
        NotificationConfig {
            enabled: false,
            backend: NotificationBackend::None,
            webhook_url: None,
            email: None,
        }
    }
}

impl Config {
    /// Returns the default configuration file path
    ///
    /// Default location: `~/.putiosync/config.toml`
    ///
    /// Can be overridden by:
    /// - Environment variable: `PUTIO_SYNC_CONFIG`
    /// - CLI flag: `--config <path>`
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use putio_sync_core::config::Config;
    ///
    /// let path = Config::default_path().unwrap();
    /// println!("Config path: {}", path.display());
    /// ```
    pub fn default_path() -> CoreResult<PathBuf> {
        // Check environment variable first
        if let Ok(env_path) = std::env::var("PUTIO_SYNC_CONFIG") {
            return Ok(PathBuf::from(env_path));
        }

        // Use home directory
        let home = dirs::home_dir().ok_or_else(|| {
            CoreError::ConfigError("Could not determine home directory".to_string())
        })?;

        Ok(home.join(".putiosync").join("config.toml"))
    }

    /// Load configuration from a file
    ///
    /// Creates the parent directory if it doesn't exist.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the configuration file
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use putio_sync_core::config::Config;
    /// use std::path::PathBuf;
    ///
    /// let config = Config::load(&PathBuf::from("config.toml")).unwrap();
    /// ```
    pub fn load<P: AsRef<Path>>(path: P) -> CoreResult<Self> {
        let path = path.as_ref();
        eprintln!("🔍 Config::load - checking path: {}", path.display());

        if !path.exists() {
            eprintln!("❌ Config file does not exist: {}", path.display());
            return Err(CoreError::config_not_found(path));
        }
        eprintln!("✅ Config file exists");

        eprintln!("📖 Reading config file contents...");
        let contents = fs::read_to_string(path).map_err(|e| {
            eprintln!("❌ Failed to read config file: {}", e);
            CoreError::ConfigError(format!("Failed to read config file: {}", e))
        })?;

        eprintln!("📝 Config file size: {} bytes", contents.len());
        eprintln!("🔧 Parsing TOML configuration...");
        let mut config: Config = toml::from_str(&contents).map_err(|e| {
            eprintln!("❌ Failed to parse TOML: {}", e);
            e
        })?;

        eprintln!("✅ TOML parsing successful");
        eprintln!("📁 Expanding paths in configuration...");
        // Automatically expand paths during loading
        config.expand_paths().map_err(|e| {
            eprintln!("❌ Path expansion failed: {}", e);
            e
        })?;

        eprintln!("✅ Configuration loaded and processed successfully");
        Ok(config)
    }

    /// Save configuration to a file
    ///
    /// Creates the parent directory if it doesn't exist.
    /// Sets file permissions to 0600 (owner read/write only) on Unix systems.
    ///
    /// # Arguments
    ///
    /// * `path` - Path where the configuration should be saved
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use putio_sync_core::config::Config;
    /// use std::path::PathBuf;
    ///
    /// let config = Config::default();
    /// config.save(&PathBuf::from("config.toml")).unwrap();
    /// ```
    pub fn save<P: AsRef<Path>>(&self, path: P) -> CoreResult<()> {
        let path = path.as_ref();

        // Create parent directory if it doesn't exist
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent).map_err(|e| {
                    CoreError::ConfigError(format!("Failed to create config directory: {}", e))
                })?;
            }
        }

        // Serialize config to TOML
        let contents = toml::to_string_pretty(self)
            .map_err(|e| CoreError::ConfigError(format!("Failed to serialize config: {}", e)))?;

        // Write to file
        fs::write(path, contents)
            .map_err(|e| CoreError::ConfigError(format!("Failed to write config file: {}", e)))?;

        // Set file permissions to 0600 on Unix systems
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let permissions = fs::Permissions::from_mode(0o600);
            fs::set_permissions(path, permissions).map_err(|e| {
                CoreError::ConfigError(format!("Failed to set config file permissions: {}", e))
            })?;
        }

        Ok(())
    }

    /// Save the settings to `path`, editing only the keys whose value changed. Comments,
    /// blank lines, ordering and keys this version doesn't know stay as they are.
    ///
    /// The credentials are never written from memory: `[auth]` and `api.auth_token` stay
    /// exactly as they are on disk, or absent when the file has none. Used by the settings
    /// routes. In memory, `api.auth_token` may hold the `PUTIO_SYNC_API_TOKEN` override,
    /// which must never be written, and `[auth]` may be older than a `login` run since
    /// startup.
    ///
    /// A key changed when its value differs from what the file holds once loaded (paths
    /// expanded, defaults filled in), so an unchanged `~/...` path keeps its `~`. A key
    /// the config no longer writes (an option set back to none) is removed.
    ///
    /// A file that can't be read or parsed is left alone; the error names no file content,
    /// so no secret leaks into it. The write goes to a temp file renamed over the
    /// original, which keeps its mode (a new file is 0600).
    pub fn save_keeping_file_credentials<P: AsRef<Path>>(&self, path: P) -> CoreResult<()> {
        let path = path.as_ref();
        let unreadable = |what: &str| {
            CoreError::ConfigError(format!(
                "Failed to {what} {}; settings not saved",
                path.display()
            ))
        };
        let after = settings_table(self)?;

        if !path.exists() {
            let contents = toml::to_string_pretty(&after).map_err(|e| {
                CoreError::ConfigError(format!("Failed to serialize config: {}", e))
            })?;
            return write_atomically(path, &contents, None);
        }

        // Write through a symlink to the file it points at
        let path = fs::canonicalize(path).map_err(|_| unreadable("read"))?;
        let contents = fs::read_to_string(&path).map_err(|_| unreadable("read"))?;
        let mut document: toml_edit::DocumentMut =
            contents.parse().map_err(|_| unreadable("parse"))?;
        let mut on_disk: Config = toml::from_str(&contents).map_err(|_| unreadable("parse"))?;
        on_disk.expand_paths().map_err(|_| unreadable("parse"))?;
        let before = settings_table(&on_disk)?;

        edit_table(document.as_table_mut(), &before, &after);
        let permissions = fs::metadata(&path).ok().map(|m| m.permissions());
        write_atomically(&path, &document.to_string(), permissions)
    }

    /// Validate the configuration
    ///
    /// Checks that all required fields are present and valid.
    ///
    /// # Examples
    ///
    /// ```
    /// use putio_sync_core::config::Config;
    ///
    /// let config = Config::default();
    /// assert!(config.validate().is_ok());
    /// ```
    pub fn expand_paths(&mut self) -> CoreResult<()> {
        // Expand download target directory
        self.download.target_directory = expand_path(&self.download.target_directory)?;

        // Expand sync local path
        self.sync.local_path = expand_path(&self.sync.local_path)?;

        // Expand watch paths
        let mut expanded_watch_paths = Vec::new();
        for path in &self.watch.paths {
            expanded_watch_paths.push(expand_path(path)?);
        }
        self.watch.paths = expanded_watch_paths;

        // The daemon moves uploaded watch files here (`~/processed` in the README)
        if let Some(archive) = &self.watch.archive_directory {
            self.watch.archive_directory = Some(expand_path(archive)?);
        }

        Ok(())
    }

    /// Validate the configuration
    ///
    /// This validates the logical consistency of the configuration
    /// and directory paths
    ///
    /// # Example
    ///
    /// ```no_run
    pub fn validate(&self) -> CoreResult<()> {
        // Validate workers is at least 1
        if self.download.workers == 0 {
            return Err(CoreError::ConfigError(
                "workers must be at least 1".to_string(),
            ));
        }

        // Validate buffer_size_mb is at least 1
        if self.download.buffer_size_mb == 0 {
            return Err(CoreError::ConfigError(
                "buffer_size_mb must be at least 1".to_string(),
            ));
        }

        // Validate chunk_size_mb is reasonable if set
        if let Some(chunk_size) = self.download.chunk_size_mb {
            if chunk_size == 0 {
                return Err(CoreError::ConfigError(
                    "chunk_size_mb must be greater than 0 if set".to_string(),
                ));
            }
            if chunk_size > 10240 {
                // 10GB max chunk size
                return Err(CoreError::ConfigError(
                    "chunk_size_mb must not exceed 10240 (10GB)".to_string(),
                ));
            }
        }

        // Validate refresh_cycle is positive
        if self.watch.refresh_cycle <= 0 {
            return Err(CoreError::ConfigError(
                "refresh_cycle must be greater than 0".to_string(),
            ));
        }

        // Validate watch paths exist if watch is enabled
        if self.watch.enabled {
            for path in &self.watch.paths {
                if !path.exists() {
                    return Err(CoreError::ConfigError(format!(
                        "Watch path does not exist: {}",
                        path.display()
                    )));
                }
                if !path.is_dir() {
                    return Err(CoreError::ConfigError(format!(
                        "Watch path is not a directory: {}",
                        path.display()
                    )));
                }
            }
        }

        // Validate watch parent_folder_id is non-negative if set
        if let Some(parent_id) = self.watch.parent_folder_id {
            if parent_id < 0 {
                return Err(CoreError::ConfigError(
                    "watch parent_folder_id must be non-negative".to_string(),
                ));
            }
        }

        // Validate remote_parent_id is non-negative if set
        if let Some(parent_id) = self.sync.remote_parent_id {
            if parent_id < 0 {
                return Err(CoreError::ConfigError(
                    "remote_parent_id must be non-negative".to_string(),
                ));
            }
        }

        self.cleanup.validate()?;
        self.arr.validate()?;

        Ok(())
    }

    /// Create a new config builder
    ///
    /// # Examples
    ///
    /// ```
    /// use putio_sync_core::config::Config;
    /// use std::path::PathBuf;
    ///
    /// let config = Config::builder()
    ///     .local_path(PathBuf::from("/downloads"))
    ///     .max_concurrent(5)
    ///     .build();
    /// ```
    pub fn builder() -> ConfigBuilder {
        ConfigBuilder::default()
    }
}

/// Builder for creating Config instances
#[derive(Default)]
pub struct ConfigBuilder {
    auth: Option<AuthConfig>,
    sync: SyncConfig,
    download: DownloadConfig,
    watch: WatchConfig,
}

impl ConfigBuilder {
    /// Set the authentication token (OAuth)
    pub fn token(mut self, token: String) -> Self {
        self.auth = Some(AuthConfig::with_token(token));
        self
    }

    /// Set authentication with username and app-specific password
    pub fn credentials(mut self, username: String, password: String) -> Self {
        self.auth = Some(AuthConfig::with_app_password(username, password));
        self
    }

    /// Set the authentication method
    pub fn auth_method(mut self, method: AuthMethod) -> Self {
        if let Some(auth) = &mut self.auth {
            auth.auth_method = method;
        }
        self
    }

    /// Set username (for AppPassword authentication)
    pub fn username(mut self, username: String) -> Self {
        if let Some(auth) = &mut self.auth {
            auth.username = Some(username);
        } else {
            self.auth = Some(AuthConfig {
                auth_method: AuthMethod::AppPassword,
                token: None,
                username: Some(username),
                password: None,
            });
        }
        self
    }

    /// Set password (for AppPassword authentication)
    pub fn password(mut self, password: String) -> Self {
        if let Some(auth) = &mut self.auth {
            auth.password = Some(password);
        } else {
            self.auth = Some(AuthConfig {
                auth_method: AuthMethod::AppPassword,
                token: None,
                username: None,
                password: Some(password),
            });
        }
        self
    }

    /// Set the local sync path
    pub fn local_path(mut self, path: PathBuf) -> Self {
        self.sync.local_path = path;
        self
    }

    /// Set the remote parent folder ID
    pub fn remote_parent_id(mut self, id: i64) -> Self {
        self.sync.remote_parent_id = Some(id);
        self
    }

    /// Set whether to delete files after download
    pub fn delete_after_download(mut self, delete: bool) -> Self {
        self.sync.delete_after_download = delete;
        self
    }

    /// Set the number of concurrent download workers
    pub fn workers(mut self, workers: usize) -> Self {
        self.download.workers = workers;
        self
    }

    /// Set the download target directory
    pub fn target_directory(mut self, path: PathBuf) -> Self {
        self.download.target_directory = path;
        self
    }

    /// Set the buffer size in MB for streaming downloads
    pub fn buffer_size_mb(mut self, size: usize) -> Self {
        self.download.buffer_size_mb = size;
        self
    }

    /// Set whether to skip existing downloads
    pub fn skip_existing(mut self, skip: bool) -> Self {
        self.download.skip_existing = skip;
        self
    }

    /// Set whether to delete from Put.io after download
    pub fn delete_after_download_config(mut self, delete: bool) -> Self {
        self.download.delete_after_download = delete;
        self
    }

    /// Set the progress display style
    pub fn progress_style(mut self, style: ProgressStyle) -> Self {
        self.download.progress_style = style;
        self
    }

    /// Set the sort order for downloads
    pub fn sort_order(mut self, order: SortOrder) -> Self {
        self.download.sort_order = order;
        self
    }

    /// Set whether to reverse the sort order
    pub fn reverse_sort(mut self, reverse: bool) -> Self {
        self.download.reverse_sort = reverse;
        self
    }

    /// Set the maximum number of concurrent downloads (deprecated, use workers instead)
    #[deprecated(since = "0.2.0", note = "Use `workers` instead")]
    pub fn max_concurrent(mut self, max: usize) -> Self {
        self.download.workers = max;
        self.download.max_concurrent = Some(max);
        self
    }

    /// Set the chunk size in MB
    pub fn chunk_size_mb(mut self, size: usize) -> Self {
        self.download.chunk_size_mb = Some(size);
        self
    }

    /// Enable or disable resume support
    pub fn resume_enabled(mut self, enabled: bool) -> Self {
        self.download.resume_enabled = enabled;
        self
    }

    /// Set whitelist patterns
    pub fn whitelist_patterns(mut self, patterns: String) -> Self {
        self.download.whitelist_patterns = Some(patterns);
        self
    }

    /// Set blacklist patterns
    pub fn blacklist_patterns(mut self, patterns: String) -> Self {
        self.download.blacklist_patterns = Some(patterns);
        self
    }

    /// Set bandwidth limit in bytes per second
    pub fn bandwidth_limit_bytes_per_sec(mut self, limit: u64) -> Self {
        self.download.bandwidth_limit_bytes_per_sec = Some(limit);
        self
    }

    /// Set maximum number of retry attempts
    pub fn max_retries(mut self, retries: u32) -> Self {
        self.download.max_retries = retries;
        self
    }

    /// Set initial retry delay in seconds
    pub fn retry_initial_delay_secs(mut self, delay: u64) -> Self {
        self.download.retry_initial_delay_secs = delay;
        self
    }

    /// Enable or disable watch mode
    pub fn watch_enabled(mut self, enabled: bool) -> Self {
        self.watch.enabled = enabled;
        self
    }

    /// Set the paths to watch
    pub fn watch_paths(mut self, paths: Vec<PathBuf>) -> Self {
        self.watch.paths = paths;
        self
    }

    /// Set the refresh cycle in seconds
    pub fn refresh_cycle(mut self, seconds: i8) -> Self {
        self.watch.refresh_cycle = seconds;
        self
    }

    /// Set the parent folder ID for watch mode uploads
    pub fn watch_parent_folder_id(mut self, id: i64) -> Self {
        self.watch.parent_folder_id = Some(id);
        self
    }

    /// Build the Config
    pub fn build(self) -> Config {
        Config {
            auth: self.auth,
            sync: self.sync,
            download: self.download,
            watch: self.watch,
            notifications: NotificationConfig::default(),
            api: ApiConfig::default(),
            logging: crate::logging::LoggingConfig::default(),
            metrics: MetricsConfig::default(),
            cleanup: CleanupConfig::default(),
            transfers: TransfersConfig::default(),
            arr: ArrConfig::default(),
            doctor: DoctorConfig::default(),
        }
    }
}

/// `[metrics]` settings as written in the config file. Values are kept as written and
/// checked by [`Config::resolve_metrics`], so a bad value gets the same error from TOML
/// and from the environment.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(default)]
pub struct MetricsConfig {
    /// Start the metrics listener
    pub enabled: bool,
    /// Listen address, `ip:port`
    pub bind: String,
    /// Scrape path
    pub path: String,
    /// `basic` or `detailed`
    pub detail: String,
}

impl Default for MetricsConfig {
    fn default() -> Self {
        let d = MetricsSettings::default();
        Self {
            enabled: d.enabled,
            bind: d.bind.to_string(),
            path: d.path,
            detail: d.detail.as_str().to_string(),
        }
    }
}

/// Largest `min_age_hours` accepted: one year
pub const MAX_CLEANUP_MIN_AGE_HOURS: u64 = 8760;

/// `[cleanup]`: removing empty folders from the Put.io account. The daemon runs it once
/// a day at `run_at` (local time) when `enabled`; manual runs ignore `enabled`.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(default)]
pub struct CleanupConfig {
    /// Run the cleanup once a day in the daemon
    pub enabled: bool,
    /// Daily run time, 24h `HH:MM` local time
    pub run_at: String,
    /// Folders created less than this many hours ago are kept
    pub min_age_hours: u64,
    /// Also remove folders whose sub-folders are all empty, in the same run. Off: only
    /// folders with nothing in them go (their parents on a later run).
    pub nested: bool,
}

impl Default for CleanupConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            run_at: "03:00".to_string(),
            min_age_hours: 24,
            nested: true,
        }
    }
}

impl CleanupConfig {
    /// `run_at` as a time of day; an error names the bad value
    pub fn run_time(&self) -> CoreResult<chrono::NaiveTime> {
        parse_run_at(&self.run_at)
    }

    /// Check `run_at` and `min_age_hours`
    pub fn validate(&self) -> CoreResult<()> {
        self.run_time()?;
        if self.min_age_hours > MAX_CLEANUP_MIN_AGE_HOURS {
            return Err(CoreError::ConfigError(format!(
                "cleanup min_age_hours must not exceed {}",
                MAX_CLEANUP_MIN_AGE_HOURS
            )));
        }
        Ok(())
    }
}

/// Parse a 24h `HH:MM` time, two digits each
fn parse_run_at(value: &str) -> CoreResult<chrono::NaiveTime> {
    let bad = || {
        CoreError::ConfigError(format!(
            "invalid cleanup run_at '{}': expected 24h HH:MM, e.g. 03:00",
            value
        ))
    };
    let (h, m) = value.split_once(':').ok_or_else(bad)?;
    if h.len() != 2 || m.len() != 2 {
        return Err(bad());
    }
    let h: u32 = h.parse().map_err(|_| bad())?;
    let m: u32 = m.parse().map_err(|_| bad())?;
    chrono::NaiveTime::from_hms_opt(h, m, 0).ok_or_else(bad)
}

/// A resolved setting and where it came from. `source` is `Env` when any environment
/// variable overrode the config file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved<T> {
    /// The effective value
    pub value: T,
    /// Where it came from
    pub source: LevelSource,
}

fn invalid(setting: &str, value: &str, allowed: &str) -> CoreError {
    CoreError::ConfigError(format!(
        "invalid {} '{}': expected one of {}",
        setting, value, allowed
    ))
}

impl Config {
    /// Resolve `[metrics]`, applying the `PUTIO_SYNC_METRICS_*` overrides. Invalid
    /// values in either place are an error, so startup fails.
    pub fn resolve_metrics(&self) -> CoreResult<Resolved<MetricsSettings>> {
        let m = &self.metrics;
        let mut source = LevelSource::Config;
        let mut env = |name: &str| {
            let v = std::env::var(name).ok();
            if v.is_some() {
                source = LevelSource::Env;
            }
            v
        };

        let enabled = match env("PUTIO_SYNC_METRICS_ENABLED") {
            None => m.enabled,
            Some(v) => match v.as_str() {
                "true" | "1" => true,
                "false" | "0" => false,
                _ => return Err(invalid("metrics.enabled", &v, "true, false, 1, 0")),
            },
        };
        let bind_str = env("PUTIO_SYNC_METRICS_BIND").unwrap_or_else(|| m.bind.clone());
        let bind = bind_str
            .parse()
            .map_err(|_| invalid("metrics.bind", &bind_str, "ip:port"))?;
        let path = env("PUTIO_SYNC_METRICS_PATH").unwrap_or_else(|| m.path.clone());
        if !path.starts_with('/') || path.chars().any(char::is_whitespace) {
            return Err(invalid(
                "metrics.path",
                &path,
                "a path starting with / and no spaces",
            ));
        }
        let detail_str = env("PUTIO_SYNC_METRICS_DETAIL").unwrap_or_else(|| m.detail.clone());
        let detail: Detail = detail_str.parse().map_err(CoreError::ConfigError)?;

        Ok(Resolved {
            value: MetricsSettings {
                enabled,
                bind,
                path,
                detail,
            },
            source,
        })
    }
}

impl Config {
    /// Resolve `[api]`, applying the `PUTIO_SYNC_API_*` overrides. Invalid values in
    /// either place are an error, so startup fails. Token errors never echo the value.
    pub fn resolve_api(&self) -> CoreResult<Resolved<ApiSettings>> {
        let a = &self.api;
        let mut source = LevelSource::Config;
        let mut env = |name: &str| {
            let v = std::env::var(name).ok();
            if v.is_some() {
                source = LevelSource::Env;
            }
            v
        };

        let listen_str = match env("PUTIO_SYNC_API_LISTEN") {
            Some(v) => Some(v),
            None => match &a.listen {
                None => None,
                Some(Flag::Bool(b)) => Some(b.to_string()),
                Some(Flag::Text(t)) => Some(t.clone()),
            },
        };
        let listen = match listen_str.as_deref() {
            None | Some("true") | Some("1") => true,
            Some("false") | Some("0") => false,
            Some(v) => return Err(invalid("api.listen", v, "true, false, 1, 0")),
        };
        let bind_str = env("PUTIO_SYNC_API_BIND")
            .or_else(|| a.bind.clone())
            .unwrap_or_else(|| DEFAULT_API_BIND.to_string());
        let bind = bind_str
            .parse()
            .map_err(|_| invalid("api.bind", &bind_str, "ip:port"))?;
        let token = env("PUTIO_SYNC_API_TOKEN").or_else(|| a.auth_token.clone());
        if let Some(t) = &token {
            if t.is_empty() || t.chars().any(char::is_whitespace) {
                return Err(CoreError::ConfigError(
                    "invalid api.auth_token: expected a non-empty value without whitespace"
                        .to_string(),
                ));
            }
        }

        Ok(Resolved {
            value: ApiSettings {
                listen,
                bind,
                token,
            },
            source,
        })
    }
}

impl Config {
    /// Resolve `[arr]`, applying `PUTIO_SYNC_ARR_PASSWORD` and
    /// `PUTIO_SYNC_RADARR_API_KEY` / `PUTIO_SYNC_SONARR_API_KEY` (the first server of that
    /// kind). An empty variable counts as unset. Errors never echo a secret.
    pub fn resolve_arr(&self) -> CoreResult<ArrSettings> {
        self.arr.validate()?;
        let env = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());

        let password = env("PUTIO_SYNC_ARR_PASSWORD")
            .or_else(|| self.arr.password.clone())
            .filter(|p| !p.is_empty());
        let mut servers = self.arr.servers.clone();
        for (kind, var) in [
            (ArrKind::Radarr, "PUTIO_SYNC_RADARR_API_KEY"),
            (ArrKind::Sonarr, "PUTIO_SYNC_SONARR_API_KEY"),
        ] {
            if let Some(key) = env(var) {
                if let Some(server) = servers.iter_mut().find(|s| s.kind == kind) {
                    server.api_key = Some(key);
                }
            }
        }
        for server in &mut servers {
            server.url = server.url.trim_end_matches('/').to_string();
            // validate() has checked it
            server.category = normalize_arr_category(&server.category).unwrap_or_default();
            if server.api_key.as_deref() == Some("") {
                server.api_key = None;
            }
        }

        Ok(ArrSettings {
            enabled: self.arr.enabled,
            username: self.arr.username.clone(),
            password,
            servers,
        })
    }
}

/// Poll the config file and pass its `[logging]` section to `on_change` whenever the
/// file's modified time or length changes.
///
/// Load and metadata errors are logged once per distinct file state, and polling carries
/// on. Production uses a 10 s interval, so a level change applies within 30 s.
pub fn spawn_reload_poller(
    path: PathBuf,
    interval: std::time::Duration,
    on_change: impl Fn(crate::logging::LoggingConfig) + Send + 'static,
) -> tokio::task::JoinHandle<()> {
    type Stamp = Option<(std::time::SystemTime, u64)>;

    async fn stamp(path: &Path) -> std::io::Result<Stamp> {
        let meta = tokio::fs::metadata(path).await?;
        Ok(Some((meta.modified()?, meta.len())))
    }

    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        // The state at startup is the one already applied.
        ticker.tick().await;
        let mut last: Stamp = stamp(&path).await.unwrap_or(None);
        let mut missing = false;

        loop {
            ticker.tick().await;
            let current = match stamp(&path).await {
                Ok(current) => current,
                Err(e) => {
                    if !missing {
                        tracing::error!(path = %path.display(), error = %e, "Config file unreadable; logging settings not reloaded");
                        missing = true;
                    }
                    last = None;
                    continue;
                }
            };
            if current == last {
                continue;
            }
            last = current;
            missing = false;

            let loaded = {
                let path = path.clone();
                tokio::task::spawn_blocking(move || Config::load(&path)).await
            };
            match loaded {
                Ok(Ok(config)) => on_change(config.logging),
                Ok(Err(e)) => {
                    tracing::error!(path = %path.display(), error = %e, "Config reload failed; logging settings not changed");
                }
                Err(e) => {
                    tracing::error!(error = %e, "Config reload task failed");
                }
            }
        }
    })
}

/// `config` as a TOML table without the credentials (`[auth]`, `api.auth_token`), which
/// the settings save never writes
fn settings_table(config: &Config) -> CoreResult<toml::Table> {
    let mut table = toml::Table::try_from(config)
        .map_err(|e| CoreError::ConfigError(format!("Failed to serialize config: {}", e)))?;
    table.remove("auth");
    // No settings route edits `[arr]`, so it keeps its password and API keys as on disk
    table.remove("arr");
    if let Some(toml::Value::Table(api)) = table.get_mut("api") {
        api.remove("auth_token");
    }
    Ok(table)
}

/// Bring `document` from `before` to `after`, touching only the keys that differ. Keys
/// in neither (unknown to this version) are left alone.
fn edit_table(document: &mut toml_edit::Table, before: &toml::Table, after: &toml::Table) {
    let empty = toml::Table::new();
    for (key, new) in after {
        let old = before.get(key);
        if old == Some(new) {
            continue;
        }
        if let toml::Value::Table(new_table) = new {
            let old_table = match old {
                Some(toml::Value::Table(table)) => table,
                _ => &empty,
            };
            match document.get_mut(key) {
                Some(toml_edit::Item::Table(table)) => {
                    edit_table(table, old_table, new_table);
                    continue;
                }
                None => {
                    let mut table = toml_edit::Table::new();
                    edit_table(&mut table, &empty, new_table);
                    document.insert(key, toml_edit::Item::Table(table));
                    continue;
                }
                // `api = { ... }`: edited key by key too, so its token stays
                Some(toml_edit::Item::Value(toml_edit::Value::InlineTable(inline))) => {
                    let mut table = inline.clone().into_table();
                    edit_table(&mut table, old_table, new_table);
                    let mut edited = table.into_inline_table();
                    *edited.decor_mut() = inline.decor().clone();
                    *inline = edited;
                    continue;
                }
                // Any other value: replaced whole below
                Some(_) => {}
            }
        }
        let mut value = edit_value(new);
        match document.get_mut(key) {
            Some(toml_edit::Item::Value(existing)) => {
                // Keep the spacing and any trailing comment
                *value.decor_mut() = existing.decor().clone();
                *existing = value;
            }
            _ => {
                document.insert(key, toml_edit::Item::Value(value));
            }
        }
    }
    for key in before.keys() {
        if !after.contains_key(key) {
            document.remove(key);
        }
    }
}

/// A `toml` value as a `toml_edit` one (tables become inline tables)
fn edit_value(value: &toml::Value) -> toml_edit::Value {
    match value {
        toml::Value::String(s) => s.as_str().into(),
        toml::Value::Integer(i) => (*i).into(),
        toml::Value::Float(f) => (*f).into(),
        toml::Value::Boolean(b) => (*b).into(),
        toml::Value::Datetime(d) => (*d).into(),
        toml::Value::Array(items) => {
            toml_edit::Value::Array(items.iter().map(edit_value).collect())
        }
        toml::Value::Table(table) => toml_edit::Value::InlineTable(
            table
                .iter()
                .map(|(k, v)| (k.as_str(), edit_value(v)))
                .collect(),
        ),
    }
}

/// Write `contents` to a temp file next to `path` and rename it over `path`, so a reader
/// never sees half a file. The file gets `permissions`, or 0600 on Unix when `None`.
fn write_atomically(
    path: &Path,
    contents: &str,
    permissions: Option<fs::Permissions>,
) -> CoreResult<()> {
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);

    let failed =
        |e: std::io::Error| CoreError::ConfigError(format!("Failed to write config file: {}", e));
    let dir = match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir,
        _ => Path::new("."),
    };
    fs::create_dir_all(dir)
        .map_err(|e| CoreError::ConfigError(format!("Failed to create config directory: {}", e)))?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "config.toml".to_string());
    let temp = dir.join(format!(
        ".{name}.{}.{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));

    let write = || -> std::io::Result<()> {
        let mut options = fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp)?;
        #[cfg(unix)]
        let permissions = permissions.or_else(|| {
            use std::os::unix::fs::PermissionsExt;
            Some(fs::Permissions::from_mode(0o600))
        });
        if let Some(permissions) = permissions {
            file.set_permissions(permissions)?;
        }
        file.write_all(contents.as_bytes())?;
        file.sync_all()?;
        fs::rename(&temp, path)
    };
    write().map_err(|e| {
        let _ = fs::remove_file(&temp);
        failed(e)
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn transfers_section_defaults_and_overrides() {
        let config: super::Config = toml::from_str("").unwrap();
        assert_eq!(config.transfers, super::TransfersConfig::default());
        assert_eq!(config.transfers.stall_threshold_hours, 6);
        assert_eq!(config.transfers.stall_check_interval_secs, 600);

        let config: super::Config =
            toml::from_str("[transfers]\nstall_threshold_hours = 12\n").unwrap();
        assert_eq!(config.transfers.stall_threshold_hours, 12);
        assert_eq!(config.transfers.stall_check_interval_secs, 600);
    }

    use super::*;
    use tempfile::TempDir;

    fn create_test_config() -> Config {
        Config {
            auth: Some(AuthConfig::with_token("test_token_123".to_string())),
            sync: SyncConfig {
                local_path: PathBuf::from("/tmp/putio"),
                remote_parent_id: Some(0),
                delete_after_download: true,
            },
            download: DownloadConfig {
                workers: 3,
                target_directory: PathBuf::from("/tmp/putio"),
                max_concurrent: Some(3),
                buffer_size_mb: 16,
                chunk_size_mb: Some(256),
                resume_enabled: true,
                skip_existing: false,
                delete_after_download: false,
                progress_style: ProgressStyle::PerFile,
                sort_order: SortOrder::ByDate,
                reverse_sort: false,
                whitelist_patterns: Some("*.mkv".to_string()),
                blacklist_patterns: Some("*.exe".to_string()),
                bandwidth_limit_bytes_per_sec: None,
                max_retries: 3,
                retry_initial_delay_secs: 1,
                delete_empty_folders: true,
                blocked_extensions: default_blocked_extensions(),
            },
            watch: WatchConfig {
                enabled: false,
                paths: vec![PathBuf::from("/remote/movies")],
                refresh_cycle: 60,
                parent_folder_id: Some(0),
                archive_directory: None,
                delete_after_upload: false,
            },
            notifications: NotificationConfig::default(),
            api: ApiConfig::default(),
            logging: crate::logging::LoggingConfig::default(),
            metrics: MetricsConfig::default(),
            cleanup: CleanupConfig::default(),
            transfers: TransfersConfig::default(),
            arr: ArrConfig::default(),
            doctor: DoctorConfig::default(),
        }
    }

    #[test]
    fn test_default_path() {
        let path = Config::default_path().unwrap();
        assert!(path.to_string_lossy().contains(".putiosync"));
        assert!(path.to_string_lossy().ends_with("config.toml"));
    }

    #[test]
    fn test_save_and_load_config() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.toml");

        let config = create_test_config();

        // Save config
        config.save(&config_path).unwrap();
        assert!(config_path.exists());

        // Load config
        let loaded = Config::load(&config_path).unwrap();

        // Verify loaded config matches
        let auth = loaded.auth.as_ref().unwrap();
        assert_eq!(auth.auth_method, AuthMethod::Token);
        assert_eq!(auth.token.as_ref().unwrap(), "test_token_123");
        assert_eq!(loaded.sync.local_path, PathBuf::from("/tmp/putio"));
        assert_eq!(loaded.sync.remote_parent_id, Some(0));
        assert!(loaded.sync.delete_after_download);
        assert_eq!(loaded.download.workers, 3);
        assert_eq!(loaded.download.max_concurrent, Some(3));
        assert_eq!(loaded.download.chunk_size_mb, Some(256));
        assert!(loaded.download.resume_enabled);
        assert_eq!(
            loaded.download.whitelist_patterns,
            Some("*.mkv".to_string())
        );
        assert_eq!(
            loaded.download.blacklist_patterns,
            Some("*.exe".to_string())
        );
        assert!(!loaded.watch.enabled);
        assert_eq!(loaded.watch.paths, vec![PathBuf::from("/remote/movies")]);
        assert_eq!(loaded.watch.refresh_cycle, 60);
        assert_eq!(loaded.watch.parent_folder_id, Some(0));
    }

    #[test]
    fn test_load_nonexistent_file() {
        let result = Config::load("/nonexistent/path/config.toml");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CoreError::ConfigError(_)));
    }

    #[test]
    fn test_save_creates_directory() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir
            .path()
            .join("nested")
            .join("dir")
            .join("config.toml");

        let config = create_test_config();
        config.save(&config_path).unwrap();

        assert!(config_path.exists());
        assert!(config_path.parent().unwrap().exists());
    }

    #[test]
    fn test_config_serialization() {
        let config = create_test_config();
        let toml_str = toml::to_string_pretty(&config).unwrap();

        // Verify TOML contains expected fields
        assert!(toml_str.contains("token"));
        assert!(toml_str.contains("local_path"));
        assert!(toml_str.contains("max_concurrent"));
        assert!(toml_str.contains("refresh_cycle"));
    }

    #[cfg(unix)]
    #[test]
    fn test_config_file_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.toml");

        let config = create_test_config();
        config.save(&config_path).unwrap();

        let metadata = fs::metadata(&config_path).unwrap();
        let permissions = metadata.permissions();
        assert_eq!(permissions.mode() & 0o777, 0o600);
    }

    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert!(config.auth.is_none());
        assert_eq!(config.sync.local_path, PathBuf::from("./putio-downloads"));
        assert_eq!(config.sync.remote_parent_id, Some(0));
        assert!(!config.sync.delete_after_download);
        assert_eq!(config.download.workers, 2);
        assert_eq!(config.download.max_concurrent, None);
        assert_eq!(config.download.buffer_size_mb, 16);
        assert_eq!(config.download.chunk_size_mb, None);
        assert!(config.download.resume_enabled);
        assert!(config.download.skip_existing);
        assert!(!config.download.delete_after_download);
        assert_eq!(config.download.progress_style, ProgressStyle::PerFile);
        assert_eq!(config.download.sort_order, SortOrder::ByDate);
        assert!(!config.download.reverse_sort);
        assert!(!config.watch.enabled);
        assert_eq!(config.watch.paths.len(), 0);
        assert_eq!(config.watch.refresh_cycle, 60);
        assert_eq!(config.watch.parent_folder_id, Some(0));
    }

    #[test]
    fn test_config_validate_success() {
        let config = Config::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_config_validate_workers_zero() {
        let mut config = Config::default();
        config.download.workers = 0;
        let result = config.validate();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CoreError::ConfigError(_)));
    }

    #[test]
    fn test_config_validate_chunk_size_zero() {
        let mut config = Config::default();
        config.download.chunk_size_mb = Some(0);
        let result = config.validate();
        assert!(result.is_err());
    }

    #[test]
    fn test_config_validate_chunk_size_too_large() {
        let mut config = Config::default();
        config.download.chunk_size_mb = Some(20000);
        let result = config.validate();
        assert!(result.is_err());
    }

    #[test]
    fn test_config_validate_negative_refresh_cycle() {
        let mut config = Config::default();
        config.watch.refresh_cycle = -1;
        let result = config.validate();
        assert!(result.is_err());
    }

    #[test]
    fn test_config_validate_negative_parent_id() {
        let mut config = Config::default();
        config.sync.remote_parent_id = Some(-5);
        let result = config.validate();
        assert!(result.is_err());
    }

    #[test]
    #[allow(deprecated)]
    fn test_config_builder() {
        let config = Config::builder()
            .token("test_token".to_string())
            .local_path(PathBuf::from("/test"))
            .remote_parent_id(123)
            .delete_after_download(true)
            .workers(5)
            .chunk_size_mb(512)
            .resume_enabled(false)
            .whitelist_patterns("*.mp4".to_string())
            .blacklist_patterns("*.exe".to_string())
            .watch_enabled(true)
            .watch_paths(vec![PathBuf::from("/path1"), PathBuf::from("/path2")])
            .refresh_cycle(30)
            .watch_parent_folder_id(123)
            .build();

        let auth = config.auth.as_ref().unwrap();
        assert_eq!(auth.auth_method, AuthMethod::Token);
        assert_eq!(auth.token.as_ref().unwrap(), "test_token");
        assert_eq!(config.sync.local_path, PathBuf::from("/test"));
        assert_eq!(config.sync.remote_parent_id, Some(123));
        assert!(config.sync.delete_after_download);
        assert_eq!(config.download.workers, 5);
        assert_eq!(config.download.chunk_size_mb, Some(512));
        assert!(!config.download.resume_enabled);
        assert_eq!(
            config.download.whitelist_patterns,
            Some("*.mp4".to_string())
        );
        assert_eq!(
            config.download.blacklist_patterns,
            Some("*.exe".to_string())
        );
        assert!(config.watch.enabled);
        assert_eq!(
            config.watch.paths,
            vec![PathBuf::from("/path1"), PathBuf::from("/path2")]
        );
        assert_eq!(config.watch.refresh_cycle, 30);
        assert_eq!(config.watch.parent_folder_id, Some(123));
    }

    #[test]
    fn test_sync_config_default() {
        let config = SyncConfig::default();
        assert_eq!(config.local_path, PathBuf::from("./putio-downloads"));
        assert_eq!(config.remote_parent_id, Some(0));
        assert!(!config.delete_after_download);
    }

    #[test]
    fn test_download_config_default() {
        let config = DownloadConfig::default();
        assert_eq!(config.workers, 2);
        assert_eq!(config.max_concurrent, None);
        assert_eq!(config.buffer_size_mb, 16);
        assert_eq!(config.chunk_size_mb, None);
        assert!(config.resume_enabled);
        assert!(config.skip_existing);
        assert!(!config.delete_after_download);
        assert_eq!(config.progress_style, ProgressStyle::PerFile);
        assert_eq!(config.sort_order, SortOrder::ByDate);
        assert!(!config.reverse_sort);
        assert_eq!(config.whitelist_patterns, None);
        assert_eq!(config.blacklist_patterns, None);
        assert_eq!(config.bandwidth_limit_bytes_per_sec, None);
        assert_eq!(config.max_retries, 3);
        assert_eq!(config.retry_initial_delay_secs, 1);
    }

    fn download_toml_without_blocked() -> String {
        let mut value = toml::Value::try_from(DownloadConfig::default()).unwrap();
        value.as_table_mut().unwrap().remove("blocked_extensions");
        toml::to_string(&value).unwrap()
    }

    #[test]
    fn test_blocked_extensions_missing_key_uses_default() {
        let config: DownloadConfig = toml::from_str(&download_toml_without_blocked()).unwrap();
        assert_eq!(
            config.blocked_extensions,
            [
                "exe", "msi", "bat", "cmd", "com", "scr", "pif", "vbs", "js", "jar", "ps1", "lnk",
                "apk", "dmg", "pkg"
            ]
        );
    }

    #[test]
    fn test_skip_existing_missing_key_defaults_to_true() {
        let mut value = toml::Value::try_from(DownloadConfig {
            skip_existing: false,
            ..DownloadConfig::default()
        })
        .unwrap();
        value.as_table_mut().unwrap().remove("skip_existing");
        let config: DownloadConfig = toml::from_str(&toml::to_string(&value).unwrap()).unwrap();
        assert!(config.skip_existing);
    }

    #[test]
    fn test_blocked_extensions_empty_disables() {
        let toml = format!(
            "{}blocked_extensions = []\n",
            download_toml_without_blocked()
        );
        let config: DownloadConfig = toml::from_str(&toml).unwrap();
        assert!(config.blocked_extensions.is_empty());
    }

    #[test]
    fn test_blocked_extensions_replaces_default() {
        let toml = format!(
            "{}blocked_extensions = [\"iso\"]\n",
            download_toml_without_blocked()
        );
        let config: DownloadConfig = toml::from_str(&toml).unwrap();
        assert_eq!(config.blocked_extensions, ["iso"]);
    }

    #[test]
    fn test_watch_config_default() {
        let config = WatchConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.paths.len(), 0);
        assert_eq!(config.refresh_cycle, 60);
        assert_eq!(config.parent_folder_id, Some(0));
    }

    #[test]
    fn test_auth_config_with_token() {
        let auth = AuthConfig::with_token("test_token".to_string());
        assert_eq!(auth.auth_method, AuthMethod::Token);
        assert_eq!(auth.token.as_ref().unwrap(), "test_token");
        assert!(auth.username.is_none());
        assert!(auth.password.is_none());
        assert!(auth.validate().is_ok());
    }

    #[test]
    fn test_auth_config_with_app_password() {
        let auth =
            AuthConfig::with_app_password("user@example.com".to_string(), "pass123".to_string());
        assert_eq!(auth.auth_method, AuthMethod::AppPassword);
        assert!(auth.token.is_none());
        assert_eq!(auth.username.as_ref().unwrap(), "user@example.com");
        assert_eq!(auth.password.as_ref().unwrap(), "pass123");
        assert!(auth.validate().is_ok());
    }

    #[test]
    fn test_auth_config_validate_token_missing() {
        let auth = AuthConfig {
            auth_method: AuthMethod::Token,
            token: None,
            username: None,
            password: None,
        };
        assert!(auth.validate().is_err());
    }

    #[test]
    fn test_auth_config_validate_token_empty() {
        let auth = AuthConfig {
            auth_method: AuthMethod::Token,
            token: Some("".to_string()),
            username: None,
            password: None,
        };
        assert!(auth.validate().is_err());
    }

    #[test]
    fn test_auth_config_validate_username_missing() {
        let auth = AuthConfig {
            auth_method: AuthMethod::AppPassword,
            token: None,
            username: None,
            password: Some("pass".to_string()),
        };
        assert!(auth.validate().is_err());
    }

    #[test]
    fn test_auth_config_validate_password_missing() {
        let auth = AuthConfig {
            auth_method: AuthMethod::AppPassword,
            token: None,
            username: Some("user@example.com".to_string()),
            password: None,
        };
        assert!(auth.validate().is_err());
    }

    #[test]
    fn test_config_builder_credentials() {
        let config = Config::builder()
            .credentials("user@example.com".to_string(), "pass123".to_string())
            .local_path(PathBuf::from("/test"))
            .build();

        let auth = config.auth.as_ref().unwrap();
        assert_eq!(auth.auth_method, AuthMethod::AppPassword);
        assert_eq!(auth.username.as_ref().unwrap(), "user@example.com");
        assert_eq!(auth.password.as_ref().unwrap(), "pass123");
        assert!(auth.token.is_none());
    }

    #[test]
    fn test_config_builder_username_password_separate() {
        let config = Config::builder()
            .username("user@example.com".to_string())
            .password("pass123".to_string())
            .build();

        let auth = config.auth.as_ref().unwrap();
        assert_eq!(auth.auth_method, AuthMethod::AppPassword);
        assert_eq!(auth.username.as_ref().unwrap(), "user@example.com");
        assert_eq!(auth.password.as_ref().unwrap(), "pass123");
    }

    #[test]
    fn test_auth_method_default() {
        let method = AuthMethod::default();
        assert_eq!(method, AuthMethod::Token);
    }
}

#[cfg(test)]
mod metrics_tests {
    use super::*;

    /// Serialises tests that change PUTIO_SYNC_METRICS_*
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    const VARS: [&str; 7] = [
        "PUTIO_SYNC_METRICS_ENABLED",
        "PUTIO_SYNC_METRICS_BIND",
        "PUTIO_SYNC_METRICS_PATH",
        "PUTIO_SYNC_METRICS_DETAIL",
        "PUTIO_SYNC_API_LISTEN",
        "PUTIO_SYNC_API_BIND",
        "PUTIO_SYNC_API_TOKEN",
    ];

    fn with_env<R>(vars: &[(&str, &str)], f: impl FnOnce() -> R) -> R {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        for v in VARS {
            std::env::remove_var(v);
        }
        for (k, v) in vars {
            std::env::set_var(k, v);
        }
        let out = f();
        for v in VARS {
            std::env::remove_var(v);
        }
        out
    }

    fn err(config: &Config) -> String {
        match config.resolve_metrics() {
            Err(CoreError::ConfigError(msg)) => msg,
            other => panic!("expected a config error, got {:?}", other),
        }
    }

    #[test]
    fn defaults() {
        let r = with_env(&[], || Config::default().resolve_metrics().unwrap());
        assert_eq!(r.value, MetricsSettings::default());
        assert_eq!(r.source, LevelSource::Config);
    }

    #[test]
    fn toml_table_is_read() {
        let config: Config = toml::from_str(
            "[metrics]\nenabled = false\nbind = \"127.0.0.1:9100\"\ndetail = \"detailed\"\n",
        )
        .unwrap();
        let r = with_env(&[], || config.resolve_metrics().unwrap());
        assert!(!r.value.enabled);
        assert_eq!(r.value.bind.to_string(), "127.0.0.1:9100");
        assert_eq!(r.value.path, "/metrics");
        assert_eq!(r.value.detail, Detail::Detailed);
    }

    #[test]
    fn env_beats_config() {
        let mut config = Config::default();
        config.metrics.enabled = false;
        let r = with_env(
            &[
                ("PUTIO_SYNC_METRICS_ENABLED", "1"),
                ("PUTIO_SYNC_METRICS_BIND", "127.0.0.1:0"),
                ("PUTIO_SYNC_METRICS_PATH", "/prom"),
                ("PUTIO_SYNC_METRICS_DETAIL", "detailed"),
            ],
            || config.resolve_metrics().unwrap(),
        );
        assert!(r.value.enabled);
        assert_eq!(r.value.bind.to_string(), "127.0.0.1:0");
        assert_eq!(r.value.path, "/prom");
        assert_eq!(r.value.detail, Detail::Detailed);
        assert_eq!(r.source, LevelSource::Env);
    }

    #[test]
    fn invalid_env_values_give_exact_messages() {
        let config = Config::default();
        let cases = [
            (
                "PUTIO_SYNC_METRICS_ENABLED",
                "yes",
                "invalid metrics.enabled 'yes': expected one of true, false, 1, 0",
            ),
            (
                "PUTIO_SYNC_METRICS_BIND",
                "nas:9742",
                "invalid metrics.bind 'nas:9742': expected one of ip:port",
            ),
            (
                "PUTIO_SYNC_METRICS_PATH",
                "metrics",
                "invalid metrics.path 'metrics': expected one of a path starting with / and no spaces",
            ),
            (
                "PUTIO_SYNC_METRICS_PATH",
                "/my metrics",
                "invalid metrics.path '/my metrics': expected one of a path starting with / and no spaces",
            ),
            (
                "PUTIO_SYNC_METRICS_DETAIL",
                "max",
                "invalid metrics.detail 'max': expected one of basic, detailed",
            ),
        ];
        for (var, value, message) in cases {
            assert_eq!(with_env(&[(var, value)], || err(&config)), message);
        }
    }

    #[test]
    fn invalid_toml_value_gives_the_same_message() {
        let mut config = Config::default();
        config.metrics.detail = "max".to_string();
        assert_eq!(
            with_env(&[], || err(&config)),
            "invalid metrics.detail 'max': expected one of basic, detailed"
        );
    }

    fn api_err(config: &Config) -> String {
        match config.resolve_api() {
            Err(CoreError::ConfigError(msg)) => msg,
            other => panic!("expected a config error, got {:?}", other),
        }
    }

    #[test]
    fn api_defaults() {
        let r = with_env(&[], || Config::default().resolve_api().unwrap());
        assert!(r.value.listen);
        assert_eq!(r.value.bind.to_string(), "0.0.0.0:15432");
        assert_eq!(r.value.token, None);
        assert_eq!(r.source, LevelSource::Config);
    }

    #[test]
    fn api_toml_table_is_read() {
        let config: Config = toml::from_str(
            "[api]\nlisten = \"false\"\nbind = \"127.0.0.1:8080\"\nauth_token = \"t0k\"\n",
        )
        .unwrap();
        let r = with_env(&[], || config.resolve_api().unwrap());
        assert!(!r.value.listen);
        assert_eq!(r.value.bind.to_string(), "127.0.0.1:8080");
        assert_eq!(r.value.token.as_deref(), Some("t0k"));
        assert_eq!(r.source, LevelSource::Config);

        let config: Config = toml::from_str("[api]\nlisten = false\n").unwrap();
        let r = with_env(&[], || config.resolve_api().unwrap());
        assert!(!r.value.listen);
    }

    #[test]
    fn api_env_overrides() {
        let mut config = Config::default();
        config.api.auth_token = Some("from-file".to_string());
        let r = with_env(
            &[
                ("PUTIO_SYNC_API_LISTEN", "0"),
                ("PUTIO_SYNC_API_BIND", "127.0.0.1:1"),
                ("PUTIO_SYNC_API_TOKEN", "abc"),
            ],
            || config.resolve_api().unwrap(),
        );
        assert!(!r.value.listen);
        assert_eq!(r.value.bind.to_string(), "127.0.0.1:1");
        assert_eq!(r.value.token.as_deref(), Some("abc"));
        assert_eq!(r.source, LevelSource::Env);
    }

    #[test]
    fn api_invalid_values() {
        let config: Config = toml::from_str("[api]\nlisten = \"maybe\"\n").unwrap();
        assert_eq!(
            with_env(&[], || api_err(&config)),
            "invalid api.listen 'maybe': expected one of true, false, 1, 0"
        );

        assert_eq!(
            with_env(&[("PUTIO_SYNC_API_BIND", "nope")], || api_err(
                &Config::default()
            )),
            "invalid api.bind 'nope': expected one of ip:port"
        );

        let mut config = Config::default();
        config.api.auth_token = Some("has space".to_string());
        let msg = with_env(&[], || api_err(&config));
        assert_eq!(
            msg,
            "invalid api.auth_token: expected a non-empty value without whitespace"
        );
        assert!(!msg.contains("has space"));

        assert_eq!(
            with_env(&[("PUTIO_SYNC_API_TOKEN", "")], || api_err(
                &Config::default()
            )),
            "invalid api.auth_token: expected a non-empty value without whitespace"
        );
    }

    #[test]
    fn api_token_never_in_debug() {
        let mut config = Config::default();
        config.api.auth_token = Some("super-secret-value".to_string());
        let settings = with_env(&[], || config.resolve_api().unwrap()).value;
        let debug = format!("{:?}", settings);
        assert!(!debug.contains("super-secret-value"));
        assert!(debug.contains("<set>"));
    }
}

#[cfg(test)]
mod save_keeping_file_credentials_tests {
    use super::*;

    fn written(path: &Path) -> Config {
        toml::from_str(&fs::read_to_string(path).unwrap()).unwrap()
    }

    /// In memory the API token may be the `PUTIO_SYNC_API_TOKEN` override; it must not
    /// reach a file that had none
    #[test]
    fn env_only_api_token_is_not_written() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let on_disk = Config {
            auth: Some(AuthConfig::with_token("file-putio".to_string())),
            ..Default::default()
        };
        on_disk.save(&path).unwrap();

        let mut memory = on_disk.clone();
        memory.api.auth_token = Some("env-api".to_string());
        memory.download.workers = 7;
        memory.save_keeping_file_credentials(&path).unwrap();

        let saved = written(&path);
        assert_eq!(saved.download.workers, 7);
        assert_eq!(saved.api.auth_token, None);
        assert!(!fs::read_to_string(&path).unwrap().contains("env-api"));
    }

    #[test]
    fn file_tokens_are_kept() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut on_disk = Config {
            auth: Some(AuthConfig::with_token("rotated-putio".to_string())),
            ..Default::default()
        };
        on_disk.api.auth_token = Some("file-api".to_string());
        on_disk.save(&path).unwrap();

        // Memory holds the env override and a Put.io token from before a re-login
        let mut memory = on_disk.clone();
        memory.api.auth_token = Some("env-api".to_string());
        memory.auth = Some(AuthConfig::with_token("old-putio".to_string()));
        memory.save_keeping_file_credentials(&path).unwrap();

        let saved = written(&path);
        assert_eq!(saved.api.auth_token.as_deref(), Some("file-api"));
        assert_eq!(
            saved.auth.and_then(|a| a.token).as_deref(),
            Some("rotated-putio")
        );
    }

    #[test]
    fn new_file_gets_no_tokens() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut memory = Config {
            auth: Some(AuthConfig::with_token("memory-putio".to_string())),
            ..Default::default()
        };
        memory.api.auth_token = Some("env-api".to_string());
        memory.save_keeping_file_credentials(&path).unwrap();

        let saved = written(&path);
        assert_eq!(saved.api.auth_token, None);
        assert!(saved.auth.is_none());
    }

    #[test]
    fn archive_directory_is_expanded_on_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut config = Config::default();
        config.watch.archive_directory = Some(PathBuf::from("~/processed"));
        config.save(&path).unwrap();
        let archive = Config::load(&path)
            .unwrap()
            .watch
            .archive_directory
            .unwrap();
        assert!(!archive.starts_with("~"), "{}", archive.display());
        assert!(archive.ends_with("processed"));
    }

    /// `Config::default()` as written by `save`, with comments, blank lines, odd
    /// spacing on the credentials, an unknown key and an unknown table added by hand
    fn hand_edited() -> String {
        let mut config = Config {
            auth: Some(AuthConfig::with_token("file-putio".to_string())),
            ..Default::default()
        };
        config.api.auth_token = Some("file-api".to_string());
        config.watch.archive_directory = Some(PathBuf::from("/srv/processed"));
        let text = toml::to_string_pretty(&config).unwrap();
        let text = text
            .replace(
                "token = \"file-putio\"",
                "token   =   \"file-putio\"   # from `login`",
            )
            .replace("[download]\n", "[download]\n# How many at once\n\n")
            .replace(
                "\nworkers = ",
                "\nfuture_knob = \"keep me\" # not known yet\nworkers = ",
            );
        format!("# My putio-sync config\n\n{text}\n# Trailing notes\n[extra]\nnote = \"unknown table\"\n")
    }

    /// `text` as `Config::load` gives it: paths expanded
    fn loaded(text: &str) -> Config {
        let mut config: Config = toml::from_str(text).unwrap();
        config.expand_paths().unwrap();
        config
    }

    fn write_hand_edited(path: &Path) -> String {
        let text = hand_edited();
        fs::write(path, &text).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
        }
        text
    }

    /// Only the changed key is rewritten: comments, blank lines, ordering, unknown keys
    /// and the credentials stay byte for byte
    #[test]
    fn only_changed_keys_are_edited() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let before = write_hand_edited(&path);
        assert!(before.contains("\nworkers = 2\n"), "{before}");

        let mut memory = loaded(&before);
        memory.api.auth_token = Some("env-api".to_string());
        memory.auth = Some(AuthConfig::with_token("old-putio".to_string()));
        memory.download.workers = 7;
        memory.save_keeping_file_credentials(&path).unwrap();

        let after = fs::read_to_string(&path).unwrap();
        assert_eq!(after, before.replace("\nworkers = 2\n", "\nworkers = 7\n"));
    }

    #[test]
    fn unchanged_config_leaves_the_file_identical() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let before = write_hand_edited(&path);
        let memory = loaded(&before);
        memory.save_keeping_file_credentials(&path).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), before);
    }

    /// A key the config no longer serialises (an option set back to none) is removed,
    /// and a key new to the file is added to its table
    #[test]
    fn cleared_and_new_keys() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let before = write_hand_edited(&path);
        let mut memory = loaded(&before);
        memory.watch.archive_directory = None;
        memory.notifications.webhook_url = Some("http://hook.local/x".to_string());
        memory.save_keeping_file_credentials(&path).unwrap();

        let after = fs::read_to_string(&path).unwrap();
        assert!(!after.contains("archive_directory"), "{after}");
        assert!(after.contains("# How many at once"), "{after}");
        let saved = written(&path);
        assert_eq!(
            saved.notifications.webhook_url.as_deref(),
            Some("http://hook.local/x")
        );
        assert_eq!(saved.watch.archive_directory, None);
    }

    /// `~` paths are expanded in memory; an unchanged one stays as written
    #[test]
    fn unexpanded_paths_are_not_rewritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut config = Config::default();
        config.download.target_directory = PathBuf::from("~/Downloads/putio");
        config.save(&path).unwrap();
        let before = fs::read_to_string(&path).unwrap();

        let mut memory = Config::load(&path).unwrap();
        memory.download.workers = 9;
        memory.save_keeping_file_credentials(&path).unwrap();

        let after = fs::read_to_string(&path).unwrap();
        assert!(after.contains("\"~/Downloads/putio\""), "{after}");
        assert_eq!(written(&path).download.workers, 9);
        assert_eq!(
            after.lines().count(),
            before.lines().count(),
            "{before}\n---\n{after}"
        );
    }

    /// The write goes through a temp file and a rename: no leftovers, mode kept
    #[cfg(unix)]
    #[test]
    fn write_is_atomic_and_keeps_mode() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let before = write_hand_edited(&path);
        let mut memory = loaded(&before);
        memory.download.workers = 3;
        memory.save_keeping_file_credentials(&path).unwrap();

        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let names: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from("config.toml")]);

        // A new file is created 0600 too
        let fresh = dir.path().join("sub").join("config.toml");
        memory.save_keeping_file_credentials(&fresh).unwrap();
        let mode = fs::metadata(&fresh).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    /// A file that no longer parses is not overwritten, and the error shows no content
    #[test]
    fn unreadable_file_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "[api]\nauth_token = \"secret-value\n").unwrap();

        let err = Config::default()
            .save_keeping_file_credentials(&path)
            .unwrap_err()
            .to_string();
        assert!(!err.contains("secret-value"), "{err}");
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "[api]\nauth_token = \"secret-value\n"
        );
    }

    #[test]
    fn cleanup_defaults_when_section_missing() {
        let config: Config = toml::from_str("[api]\nport = 9000\n").unwrap();
        assert_eq!(config.cleanup, CleanupConfig::default());
        assert!(!config.cleanup.enabled);
        assert_eq!(config.cleanup.run_at, "03:00");
        assert_eq!(config.cleanup.min_age_hours, 24);
        assert!(config.cleanup.nested);
        assert_eq!(
            config.cleanup.run_time().unwrap(),
            chrono::NaiveTime::from_hms_opt(3, 0, 0).unwrap()
        );

        let partial: Config = toml::from_str("[cleanup]\nenabled = true\n").unwrap();
        assert!(partial.cleanup.enabled);
        assert_eq!(partial.cleanup.run_at, "03:00");
        assert!(partial.cleanup.nested);

        let flat: Config = toml::from_str("[cleanup]\nnested = false\n").unwrap();
        assert!(!flat.cleanup.nested);
    }

    #[test]
    fn cleanup_run_at_is_validated() {
        for good in ["00:00", "03:00", "23:59", "04:30"] {
            let mut config = Config::default();
            config.cleanup.run_at = good.to_string();
            assert!(config.validate().is_ok(), "{good}");
        }
        for bad in [
            "25:00", "3:00", "03:60", "0300", "", "ab:cd", "03:00:00", "-1:00",
        ] {
            let mut config = Config::default();
            config.cleanup.run_at = bad.to_string();
            let err = config.validate().unwrap_err().to_string();
            assert!(err.contains("run_at"), "{bad}: {err}");
        }
    }

    #[test]
    fn cleanup_min_age_is_bounded() {
        let mut config = Config::default();
        config.cleanup.min_age_hours = 0;
        assert!(config.validate().is_ok());
        config.cleanup.min_age_hours = MAX_CLEANUP_MIN_AGE_HOURS;
        assert!(config.validate().is_ok());
        config.cleanup.min_age_hours = MAX_CLEANUP_MIN_AGE_HOURS + 1;
        let err = config.validate().unwrap_err().to_string();
        assert!(err.contains("min_age_hours"), "{err}");
    }

    /// A file without `[cleanup]` gains the section on save; comments elsewhere stay
    #[test]
    fn cleanup_save_adds_section_and_keeps_comments() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            "# my settings\n[api]\n# not the default\nport = 9000\n",
        )
        .unwrap();

        let mut memory: Config = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        memory.cleanup.enabled = true;
        memory.cleanup.run_at = "04:30".to_string();
        memory.save_keeping_file_credentials(&path).unwrap();

        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("# my settings"), "{text}");
        assert!(text.contains("# not the default"), "{text}");
        let saved = written(&path);
        assert!(saved.cleanup.enabled);
        assert_eq!(saved.cleanup.run_at, "04:30");
        assert_eq!(saved.cleanup.min_age_hours, 24);
    }
}

#[cfg(test)]
mod arr_tests {
    use super::*;

    /// Serialises tests that change the `[arr]` env overrides
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    const VARS: [&str; 3] = [
        "PUTIO_SYNC_ARR_PASSWORD",
        "PUTIO_SYNC_RADARR_API_KEY",
        "PUTIO_SYNC_SONARR_API_KEY",
    ];

    fn with_env<R>(vars: &[(&str, &str)], f: impl FnOnce() -> R) -> R {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        for v in VARS {
            std::env::remove_var(v);
        }
        for (k, v) in vars {
            std::env::set_var(k, v);
        }
        let out = f();
        for v in VARS {
            std::env::remove_var(v);
        }
        out
    }

    const TWO_SERVERS: &str = r#"
[arr]
enabled = true
password = "file-pass"

[[arr.servers]]
kind = "sonarr"
url = "http://nas:8989/"
api_key = "file-sonarr-key"
category = "/tv/"

[[arr.servers]]
kind = "radarr"
url = "https://nas:7878"
category = "movies"
"#;

    #[test]
    fn defaults_are_off() {
        let config = Config::default();
        assert!(!config.arr.enabled);
        assert_eq!(config.arr.username, "putio-sync");
        let r = with_env(&[], || config.resolve_arr().unwrap());
        assert!(!r.enabled);
        assert_eq!(r.password, None);
        assert!(r.servers.is_empty());
    }

    #[test]
    fn file_values_are_normalised() {
        let config: Config = toml::from_str(TWO_SERVERS).unwrap();
        let r = with_env(&[], || config.resolve_arr().unwrap());
        assert!(r.enabled);
        assert_eq!(r.password.as_deref(), Some("file-pass"));
        assert_eq!(r.servers[0].url, "http://nas:8989");
        assert_eq!(r.servers[0].category, "tv");
        assert_eq!(r.servers[0].api_key.as_deref(), Some("file-sonarr-key"));
        assert_eq!(r.servers[1].api_key, None);
        assert_eq!(r.server_for("/tv").map(|s| s.kind), Some(ArrKind::Sonarr));
        assert!(r.server_for("music").is_none());
    }

    #[test]
    fn env_beats_file() {
        let config: Config = toml::from_str(TWO_SERVERS).unwrap();
        let r = with_env(
            &[
                ("PUTIO_SYNC_ARR_PASSWORD", "env-pass"),
                ("PUTIO_SYNC_RADARR_API_KEY", "env-radarr"),
                ("PUTIO_SYNC_SONARR_API_KEY", ""),
            ],
            || config.resolve_arr().unwrap(),
        );
        assert_eq!(r.password.as_deref(), Some("env-pass"));
        assert_eq!(r.servers[1].api_key.as_deref(), Some("env-radarr"));
        // An empty variable counts as unset
        assert_eq!(r.servers[0].api_key.as_deref(), Some("file-sonarr-key"));
    }

    #[test]
    fn debug_hides_secrets() {
        let config: Config = toml::from_str(TWO_SERVERS).unwrap();
        let shown = format!(
            "{:?} {:?}",
            config,
            with_env(&[], || config.resolve_arr().unwrap())
        );
        assert!(!shown.contains("file-pass"));
        assert!(!shown.contains("file-sonarr-key"));
        assert!(shown.contains("<set>"));
        assert!(shown.contains("<unset>"));
    }

    fn arr_error(toml_text: &str) -> String {
        let config: Config = toml::from_str(toml_text).unwrap();
        match config.validate() {
            Err(CoreError::ConfigError(msg)) => msg,
            other => panic!("expected a config error, got {:?}", other),
        }
    }

    #[test]
    fn bad_servers_are_rejected() {
        let server = |url: &str, category: &str| {
            format!(
                "[[arr.servers]]\nkind = \"sonarr\"\nurl = \"{url}\"\ncategory = \"{category}\"\n"
            )
        };
        assert!(arr_error(&server("nas:8989", "tv")).contains("arr server url"));
        assert!(arr_error(&server("ftp://nas", "tv")).contains("arr server url"));
        assert!(arr_error(&server("http://nas", "../tv")).contains("arr server category"));
        assert!(arr_error(&server("http://nas", "tv//x")).contains("arr server category"));
        let twice = format!("{}{}", server("http://a", "tv"), server("http://b", "/tv"));
        assert!(arr_error(&twice).contains("used twice"));
        assert!(arr_error("[arr]\nusername = \"a:b\"\n").contains("arr username"));
    }

    #[test]
    fn categories_normalise() {
        assert_eq!(normalize_arr_category("/a/b/").as_deref(), Some("a/b"));
        assert_eq!(normalize_arr_category("").as_deref(), Some(""));
        assert_eq!(normalize_arr_category("a/./b"), None);
        assert_eq!(normalize_arr_category("a\\b"), None);
    }

    /// The settings routes never write `[arr]`: its secrets stay as on disk
    #[test]
    fn settings_save_keeps_arr_section() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, TWO_SERVERS).unwrap();

        let mut memory: Config = toml::from_str(TWO_SERVERS).unwrap();
        memory.arr.password = Some("memory-pass".to_string());
        memory.arr.servers.clear();
        memory.download.workers = 9;
        memory.save_keeping_file_credentials(&path).unwrap();

        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("workers = 9"));
        assert!(!text.contains("memory-pass"));
        assert!(text.contains(TWO_SERVERS.trim()));
    }
}
