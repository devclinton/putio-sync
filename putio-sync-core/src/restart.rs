//! The settings that still need a restart.
//!
//! The daemon applies almost every setting while it runs (see [`crate::config_hub`]).
//! [`RESTART_ONLY`] lists the rest: what is read once at startup. It is the one list the
//! settings page, the status route and the import message agree on.

use crate::config::Config;

/// Config sections, and the keys in each, that are read only at startup. An empty key
/// list means every key in the section.
pub const RESTART_ONLY: &[(&str, &[&str])] = &[
    ("database", &[]),
    // Whether the API listens at all, and the remote API a client mode talks to. Where it
    // listens (`bind`, `[tailscale] tailnet_only`) and all of `[metrics]` apply live;
    // auth_token and allowed_hosts are read on each request
    ("api", &["enabled", "url", "listen"]),
    // Where logs go; the level and filters apply live
    (
        "logging",
        &[
            "file_enabled",
            "file_path",
            "console_enabled",
            "console_format",
        ],
    ),
    // The watch folders' release check, given to the daemon at startup
    ("precheck", &[]),
];

/// True when `section.key` is read only at startup
pub fn needs_restart(section: &str, key: &str) -> bool {
    RESTART_ONLY
        .iter()
        .any(|(s, keys)| *s == section && (keys.is_empty() || keys.contains(&key)))
}

/// The restart-only settings that differ between `started` (the config the process
/// started with) and `running` (the one it runs on now), as `section.key`, sorted. A
/// whole section that appeared or went away is named alone. Never holds values.
pub fn pending(started: &Config, running: &Config) -> Vec<String> {
    let (Ok(started), Ok(running)) = (
        toml::Table::try_from(started),
        toml::Table::try_from(running),
    ) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (section, keys) in RESTART_ONLY {
        match (started.get(*section), running.get(*section)) {
            (None, None) => {}
            (Some(toml::Value::Table(a)), Some(toml::Value::Table(b))) => {
                let mut names: Vec<&String> = a.keys().chain(b.keys()).collect();
                names.sort();
                names.dedup();
                for name in names {
                    let restart = keys.is_empty() || keys.contains(&name.as_str());
                    if restart && a.get(name) != b.get(name) {
                        out.push(format!("{section}.{name}"));
                    }
                }
            }
            (a, b) => {
                if a != b {
                    out.push((*section).to_string());
                }
            }
        }
    }
    out.sort();
    out
}

/// The top-level config sections that differ between `old` and `new`, sorted. Names
/// only, never values: this is what `config.changed` carries (feature 057).
pub fn changed_sections(old: &Config, new: &Config) -> Vec<String> {
    let (Ok(old), Ok(new)) = (toml::Table::try_from(old), toml::Table::try_from(new)) else {
        return Vec::new();
    };
    let mut names: Vec<&String> = old.keys().chain(new.keys()).collect();
    names.sort();
    names.dedup();
    names
        .into_iter()
        .filter(|name| old.get(*name) != new.get(*name))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn changed_sections_are_named_without_values() {
        let old = Config::default();
        let mut new = Config::default();
        assert!(changed_sections(&old, &new).is_empty());
        new.auth = Some(toml::from_str("token = \"s3cret-token\"").unwrap());
        new.api.enabled = !old.api.enabled;
        let changed = changed_sections(&old, &new);
        assert_eq!(changed, vec!["api", "auth"]);
        assert!(!changed.join(" ").contains("s3cret"));
    }

    use super::*;

    #[test]
    fn nothing_pending_for_the_same_config() {
        assert!(pending(&Config::default(), &Config::default()).is_empty());
    }

    #[test]
    fn live_changes_are_not_pending() {
        let started = Config::default();
        let mut running = Config::default();
        running.watch.refresh_cycle += 5;
        running.logging.level = "debug".to_string();
        running.api.allowed_hosts = vec!["nas.lan".to_string()];
        // The listeners move live (part 3)
        running.api.bind = Some(crate::listen::BindList::from("127.0.0.1:15432"));
        running.metrics.enabled = !started.metrics.enabled;
        running.metrics.bind = crate::listen::BindList::from("127.0.0.1:9999");
        running.tailscale.tailnet_only = true;
        // So does the Put.io token (part 4)
        running.auth = Some(crate::config::AuthConfig::with_token("new".to_string()));
        assert!(pending(&started, &running).is_empty());
    }

    #[test]
    fn restart_only_changes_are_named() {
        let started = Config::default();
        let mut running = Config::default();
        running.logging.console_enabled = !started.logging.console_enabled;
        running.api.enabled = !started.api.enabled;
        running.api.url = Some("http://nas.lan:15432".to_string());
        assert_eq!(
            pending(&started, &running),
            vec!["api.enabled", "api.url", "logging.console_enabled"]
        );
    }

    #[test]
    fn the_registry_names_real_sections_and_keys() {
        let table = toml::Table::try_from(Config::default()).unwrap();
        for (section, keys) in RESTART_ONLY {
            if keys.is_empty() {
                continue;
            }
            let Some(toml::Value::Table(fields)) = table.get(*section) else {
                panic!("[{section}] isn't in the default config");
            };
            for key in *keys {
                // Optional keys are left out when unset; a key that never serialises is a typo
                let known = fields.contains_key(*key)
                    || matches!((*section, *key), ("api", "url" | "listen"));
                assert!(known, "{section}.{key} isn't a config key");
            }
        }
        assert!(needs_restart("api", "listen"));
        assert!(!needs_restart("api", "bind"));
        assert!(!needs_restart("metrics", "enabled"));
        assert!(!needs_restart("tailscale", "tailnet_only"));
        assert!(!needs_restart("auth", "token"));
        assert!(needs_restart("database", "anything"));
        assert!(!needs_restart("api", "allowed_hosts"));
        assert!(!needs_restart("watch", "refresh_cycle"));
    }
}
