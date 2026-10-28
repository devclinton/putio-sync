//! The policy for a folder from `[precheck]` and 009 rules (T005)

use std::path::Path;

use putio_sync_core::config::{DownloadConfig, PrecheckConfig, PrecheckOverride};
use putio_sync_core::rules::{FolderRule, RuleSet};
use putio_sync_precheck::policy::Key;
use putio_sync_precheck::Policy;

fn rules(rules: Vec<FolderRule>) -> RuleSet {
    RuleSet::from_config(&DownloadConfig {
        rules,
        ..DownloadConfig::default()
    })
}

fn preset(path: &str, preset: &str) -> FolderRule {
    FolderRule {
        path: path.to_string(),
        preset: Some(preset.to_string()),
        ..FolderRule::default()
    }
}

#[test]
fn no_rules_is_video() {
    let p = Policy::for_folder(
        &PrecheckConfig::default(),
        &rules(vec![]),
        "sonarr",
        Key::Category("sonarr"),
    );
    assert!(p.enabled);
    assert!(p.wanted.contains("mkv") && p.wanted.contains("mp4"));
    assert!(!p.wanted.contains("exe"));
    assert_eq!(p.wanted_from, None);
    assert_eq!(p.kind, "video");
    assert_eq!(p.min_size, 50 * 1024 * 1024);
    assert!(!p.allow_archives);
}

#[test]
fn audio_preset_folder() {
    let p = Policy::for_folder(
        &PrecheckConfig::default(),
        &rules(vec![preset("lidarr", "audio")]),
        "lidarr/Artist",
        Key::Category("lidarr"),
    );
    assert!(p.wanted.contains("flac"));
    assert!(!p.wanted.contains("mkv"));
    assert_eq!(p.wanted_from.as_deref(), Some("lidarr"));
    assert_eq!(p.kind, "audio");
}

#[test]
fn block_list_rule_keeps_video() {
    let p = Policy::for_folder(
        &PrecheckConfig::default(),
        &rules(vec![preset("tv", "no-binaries")]),
        "tv",
        Key::Category("tv"),
    );
    assert!(p.wanted.contains("mkv"));
    assert_eq!(p.wanted_from, None);
    assert_eq!(p.kind, "video");
}

#[test]
fn custom_allow_list_minus_blocked() {
    let rule = FolderRule {
        path: "music".to_string(),
        preset: Some("audio".to_string()),
        block: vec!["wav".to_string()],
        ..FolderRule::default()
    };
    let p = Policy::for_folder(
        &PrecheckConfig::default(),
        &rules(vec![rule]),
        "music",
        Key::Category("music"),
    );
    assert!(p.wanted.contains("flac"));
    assert!(!p.wanted.contains("wav"));
}

#[test]
fn overrides_apply_last() {
    let mut cfg = PrecheckConfig::default();
    cfg.categories.insert(
        "software".to_string(),
        PrecheckOverride {
            enabled: Some(false),
            ..PrecheckOverride::default()
        },
    );
    cfg.watch.insert(
        "/watch/tv".to_string(),
        PrecheckOverride {
            min_size_mb: Some(100),
            allow_archives: Some(true),
            ..PrecheckOverride::default()
        },
    );
    let set = rules(vec![]);
    let off = Policy::for_folder(&cfg, &set, "software", Key::Category("software"));
    assert!(!off.enabled);
    let watch = Policy::for_folder(&cfg, &set, "", Key::Watch(Path::new("/watch/tv")));
    assert!(watch.enabled);
    assert_eq!(watch.min_size, 100 * 1024 * 1024);
    assert!(watch.allow_archives);
    let other = Policy::for_folder(&cfg, &set, "tv", Key::Category("tv"));
    assert!(other.enabled);
    assert_eq!(other.min_size, 50 * 1024 * 1024);
}
