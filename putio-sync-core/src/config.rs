//! Configuration management for putio-sync
//!
//! This module handles loading, saving, and managing configuration files
//! in TOML format.

use crate::error::{CoreError, CoreResult};
use crate::path_utils::expand_path;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Default value for delete_empty_folders
fn default_delete_empty_folders() -> bool {
    true
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
    /// Logging configuration
    #[serde(default)]
    pub logging: crate::logging::LoggingConfig,
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
    /// Skip files that have already been downloaded
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
            skip_existing: false,
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
    /// use putio_sync_core::config::*;
    /// use std::path::PathBuf;
    ///
    /// let config = Config {
    ///     auth: None,
    ///     sync: SyncConfig {
    ///         local_path: PathBuf::from("/tmp/putio"),
    ///         remote_parent_id: Some(0),
    ///         delete_after_download: true,
    ///     },
    ///     download: DownloadConfig {
    ///         workers: 3,
    ///         target_directory: PathBuf::from("/tmp/putio"),
    ///         max_concurrent: Some(3),
    ///         buffer_size_mb: 16,
    ///         chunk_size_mb: None,
    ///         resume_enabled: true,
    ///         skip_existing: false,
    ///         delete_after_download: false,
    ///         progress_style: ProgressStyle::PerFile,
    ///         sort_order: SortOrder::ByDate,
    ///         reverse_sort: false,
    ///         whitelist_patterns: None,
    ///         blacklist_patterns: None,
    ///         bandwidth_limit_bytes_per_sec: None,
    ///         max_retries: 3,
    ///         retry_initial_delay_secs: 1,
    ///         delete_empty_folders: true,
    ///     },
    ///     watch: WatchConfig {
    ///         enabled: false,
    ///         paths: vec![],
    ///         refresh_cycle: 60,
    ///         parent_folder_id: Some(0),
    ///         archive_directory: None,
    ///         delete_after_upload: false,
    ///     },
    ///     notifications: NotificationConfig::default(),
    /// };
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
            logging: crate::logging::LoggingConfig::default(),
        }
    }
}

#[cfg(test)]
mod tests {
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
        assert!(!config.download.skip_existing);
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
        assert!(!config.skip_existing);
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
