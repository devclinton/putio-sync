//! Configuration management for putio-sync
//!
//! This module handles loading, saving, and managing configuration files
//! in TOML format.

use crate::error::{CoreError, CoreResult};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Main configuration structure
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    /// Authentication configuration
    pub auth: Option<AuthConfig>,
    /// Sync configuration
    pub sync: SyncConfig,
    /// Download configuration
    pub download: DownloadConfig,
    /// Watch configuration
    pub watch: WatchConfig,
}

/// Authentication configuration
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AuthConfig {
    /// OAuth token for Put.io API
    pub token: String,
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

/// Download configuration
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DownloadConfig {
    /// Maximum number of concurrent downloads
    pub max_concurrent: usize,
    /// Chunk size in MB for chunked downloads (None to disable)
    pub chunk_size_mb: Option<usize>,
    /// Enable resume support for interrupted downloads
    pub resume_enabled: bool,
    /// Glob patterns to whitelist files (e.g., "*.mkv")
    pub whitelist_patterns: Option<String>,
    /// Glob patterns to blacklist files (e.g., "*.exe")
    pub blacklist_patterns: Option<String>,
}

/// Watch configuration
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WatchConfig {
    /// How often to refresh in seconds (used when file system events aren't available)
    pub refresh_cycle: i8,
    /// Paths to watch for changes
    pub paths: Vec<String>,
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
        let home = dirs::home_dir()
            .ok_or_else(|| CoreError::ConfigError("Could not determine home directory".to_string()))?;

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

        if !path.exists() {
            return Err(CoreError::ConfigError(format!(
                "Configuration file not found: {}",
                path.display()
            )));
        }

        let contents = fs::read_to_string(path)
            .map_err(|e| CoreError::ConfigError(format!("Failed to read config file: {}", e)))?;

        let config: Config = toml::from_str(&contents)?;
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
    ///         remote_parent_id: None,
    ///         delete_after_download: true,
    ///     },
    ///     download: DownloadConfig {
    ///         max_concurrent: 3,
    ///         chunk_size_mb: None,
    ///         resume_enabled: true,
    ///         whitelist_patterns: None,
    ///         blacklist_patterns: None,
    ///     },
    ///     watch: WatchConfig {
    ///         refresh_cycle: 60,
    ///         paths: vec![],
    ///     },
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_config() -> Config {
        Config {
            auth: Some(AuthConfig {
                token: "test_token_123".to_string(),
            }),
            sync: SyncConfig {
                local_path: PathBuf::from("/tmp/putio"),
                remote_parent_id: Some(0),
                delete_after_download: true,
            },
            download: DownloadConfig {
                max_concurrent: 3,
                chunk_size_mb: Some(256),
                resume_enabled: true,
                whitelist_patterns: Some("*.mkv".to_string()),
                blacklist_patterns: Some("*.exe".to_string()),
            },
            watch: WatchConfig {
                refresh_cycle: 60,
                paths: vec!["/remote/movies".to_string()],
            },
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
        assert_eq!(
            loaded.auth.as_ref().unwrap().token,
            "test_token_123"
        );
        assert_eq!(loaded.sync.local_path, PathBuf::from("/tmp/putio"));
        assert_eq!(loaded.sync.remote_parent_id, Some(0));
        assert_eq!(loaded.sync.delete_after_download, true);
        assert_eq!(loaded.download.max_concurrent, 3);
        assert_eq!(loaded.download.chunk_size_mb, Some(256));
        assert_eq!(loaded.download.resume_enabled, true);
        assert_eq!(loaded.download.whitelist_patterns, Some("*.mkv".to_string()));
        assert_eq!(loaded.download.blacklist_patterns, Some("*.exe".to_string()));
        assert_eq!(loaded.watch.refresh_cycle, 60);
        assert_eq!(loaded.watch.paths, vec!["/remote/movies"]);
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
        let config_path = temp_dir.path().join("nested").join("dir").join("config.toml");

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
}
