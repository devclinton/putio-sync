//! Every `[download]` setting the download manager uses comes from `config.toml`
//! through one conversion, shared by `daemon`, `server` and the settings API
//! (post-review fix to feature 004).

use std::path::PathBuf;

use putio_sync_core::config::DownloadConfig as DownloadSettings;
use putio_sync_download::DownloadConfig;

/// Settings with every manager field away from both crates' defaults
fn non_default_settings() -> DownloadSettings {
    DownloadSettings {
        workers: 7,
        target_directory: PathBuf::from("/srv/putio"),
        buffer_size_mb: 48,
        skip_existing: true,
        delete_after_download: true,
        bandwidth_limit_bytes_per_sec: Some(1_048_576),
        max_retries: 9,
        retry_initial_delay_secs: 11,
        blocked_extensions: vec!["exe".to_string(), "iso".to_string()],
        ..DownloadSettings::default()
    }
}

#[test]
fn every_manager_field_comes_from_settings() {
    let settings = non_default_settings();
    let config = DownloadConfig::from(&settings);
    assert_eq!(
        config,
        DownloadConfig {
            target_directory: PathBuf::from("/srv/putio"),
            skip_existing: true,
            buffer_size_mb: 48,
            bandwidth_limit_bytes_per_sec: Some(1_048_576),
            max_retries: 9,
            retry_initial_delay_secs: 11,
            workers: 7,
            delete_after_download: true,
            blocked_extensions: vec!["exe".to_string(), "iso".to_string()],
        }
    );
}

#[test]
fn defaults_follow_config_toml_defaults() {
    let settings = DownloadSettings::default();
    let config = DownloadConfig::from(&settings);
    assert_eq!(config.workers, settings.workers);
    assert!(config.skip_existing);
    assert_eq!(config.bandwidth_limit_bytes_per_sec, None);
    assert_eq!(config.max_retries, settings.max_retries);
    assert_eq!(
        config.retry_initial_delay_secs,
        settings.retry_initial_delay_secs
    );
}
