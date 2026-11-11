//! Every `[download]` setting the download manager uses comes from `config.toml`
//! through one conversion, shared by `daemon`, `server` and the settings API
//! (post-review fix to feature 004).

use std::path::PathBuf;

use putio_sync_core::config::DownloadConfig as DownloadSettings;
use putio_sync_core::rules::{FolderRule, RuleSet};
use putio_sync_download::DownloadConfig;

fn schedule_entry() -> putio_sync_core::bandwidth::ScheduleEntry {
    putio_sync_core::bandwidth::ScheduleEntry {
        days: "daily".to_string(),
        start: "01:00".to_string(),
        end: "07:00".to_string(),
        limit: putio_sync_core::bandwidth::Limit::Unlimited,
    }
}

/// Settings with every manager field away from both crates' defaults
fn non_default_settings() -> DownloadSettings {
    DownloadSettings {
        workers: 7,
        target_directory: PathBuf::from("/srv/putio"),
        buffer_size_mb: 48,
        on_existing: Some(putio_sync_core::config::ExistingFile::KeepBoth),
        resume_enabled: false,
        delete_after_download: true,
        stream_mp4: false,
        bandwidth_limit_bytes_per_sec: Some(1_048_576),
        bandwidth_schedule: vec![schedule_entry()],
        slow_limit_bytes_per_sec: 300_000,
        max_retries: 9,
        retry_initial_delay_secs: 11,
        blocked_extensions: vec!["exe".to_string(), "iso".to_string()],
        rules: vec![FolderRule {
            path: "Movies".to_string(),
            preset: Some("video".to_string()),
            ..FolderRule::default()
        }],
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
            on_existing: putio_sync_core::config::ExistingFile::KeepBoth,
            resume_enabled: false,
            buffer_size_mb: 48,
            bandwidth_limit_bytes_per_sec: Some(1_048_576),
            bandwidth_schedule: vec![schedule_entry()],
            slow_limit_bytes_per_sec: 300_000,
            max_retries: 9,
            retry_initial_delay_secs: 11,
            workers: 7,
            delete_after_download: true,
            stream_mp4: false,
            rules: RuleSet::from_config(&settings),
            verify: putio_sync_download::manager::VerifySettings::default(),
        }
    );
    assert!(config.rules.decide("", "a.iso") != putio_sync_core::rules::Decision::Download);
    assert!(config.rules.decide("Movies", "a.nfo") != putio_sync_core::rules::Decision::Download);
}

#[test]
fn defaults_follow_config_toml_defaults() {
    let settings = DownloadSettings::default();
    let config = DownloadConfig::from(&settings);
    assert_eq!(config.workers, settings.workers);
    assert_eq!(
        config.on_existing,
        putio_sync_core::config::ExistingFile::Resume
    );
    assert!(config.resume_enabled);
    assert_eq!(config.bandwidth_limit_bytes_per_sec, None);
    assert_eq!(config.max_retries, settings.max_retries);
    assert_eq!(
        config.retry_initial_delay_secs,
        settings.retry_initial_delay_secs
    );
}
