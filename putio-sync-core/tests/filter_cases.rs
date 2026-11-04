//! Feature 020: the shared filter cases, also run against `ui/query.js` by
//! `putio-sync-api/tests/ui/query.test.mjs`, so the two matchers can't drift.

use putio_sync_core::pattern::Pattern;
use serde::Deserialize;

#[derive(Deserialize)]
struct Case {
    filter: String,
    #[serde(default)]
    names: Vec<String>,
    #[serde(default)]
    paths: Vec<String>,
    shown: Vec<String>,
}

#[test]
fn shared_filter_cases() {
    let cases: Vec<Case> = serde_json::from_str(include_str!("data/filter-cases.json")).unwrap();
    assert!(cases.len() >= 40);
    for case in cases {
        let p = Pattern::parse(&case.filter);
        let shown: Vec<&String> = if case.paths.is_empty() {
            case.names.iter().filter(|n| p.matches(n, n)).collect()
        } else {
            case.paths
                .iter()
                .filter(|path| p.matches(path.rsplit('/').next().unwrap_or(path), path))
                .collect()
        };
        assert_eq!(
            shown,
            case.shown.iter().collect::<Vec<_>>(),
            "filter {:?}",
            case.filter
        );
    }
}
