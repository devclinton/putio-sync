//! Extension blocklist: files that are never downloaded (fake releases are usually executables)

use std::path::Path;

/// Default blocked extensions. Keep in sync with `putio_sync_core::config::DEFAULT_BLOCKED_EXTENSIONS`.
pub const DEFAULT_BLOCKED_EXTENSIONS: &[&str] = &[
    "exe", "msi", "bat", "cmd", "com", "scr", "pif", "vbs", "js", "jar", "ps1", "lnk", "apk",
    "dmg", "pkg",
];

/// The default list as owned strings
pub fn default_blocked_extensions() -> Vec<String> {
    DEFAULT_BLOCKED_EXTENSIONS
        .iter()
        .map(|e| e.to_string())
        .collect()
}

/// The lowercased final extension of `name` when it is in `blocked`
pub fn blocked_extension(name: &str, blocked: &[String]) -> Option<String> {
    let ext = Path::new(name).extension()?.to_str()?.to_ascii_lowercase();
    blocked
        .iter()
        .any(|b| b.trim_start_matches('.').eq_ignore_ascii_case(&ext))
        .then_some(ext)
}

/// Whether a file with this name must not be downloaded
pub fn is_blocked(name: &str, blocked: &[String]) -> bool {
    blocked_extension(name, blocked).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_blocked_matches_final_extension() {
        let list = default_blocked_extensions();
        let cases = [
            ("Show S01E02.exe", true),
            ("setup.MSI", true),
            ("a.mkv.exe", true),
            ("a.exe.mkv", false),
            ("README", false),
            (".exe", false),
        ];
        for (name, expected) in cases {
            assert_eq!(is_blocked(name, &list), expected, "{name}");
        }
        assert!(!is_blocked("Show S01E02.exe", &[]));
    }
}
