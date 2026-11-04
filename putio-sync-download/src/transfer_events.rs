//! Transfer transitions for `transfer.*` events (feature 027, research R6)
//!
//! Each fresh Put.io transfer list is compared with the one before it. The id → class map
//! of the latest list is stored in `daemon_flags`, so a restart does not send the events
//! again; with no stored map the first list is the baseline and sends nothing.

use std::collections::BTreeMap;

/// How far a transfer has got, as far as events go
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferClass {
    /// Queued, downloading or anything else not below
    Active,
    /// `COMPLETED` or `SEEDING`
    Finished,
    /// `ERROR`
    Failed,
}

impl TransferClass {
    /// The class of a Put.io transfer status
    pub fn of(status: &str) -> TransferClass {
        match status {
            "COMPLETED" | "SEEDING" => TransferClass::Finished,
            "ERROR" => TransferClass::Failed,
            _ => TransferClass::Active,
        }
    }

    fn code(self) -> char {
        match self {
            TransferClass::Active => 'a',
            TransferClass::Finished => 'f',
            TransferClass::Failed => 'x',
        }
    }

    fn from_code(code: &str) -> Option<TransferClass> {
        match code {
            "a" => Some(TransferClass::Active),
            "f" => Some(TransferClass::Finished),
            "x" => Some(TransferClass::Failed),
            _ => None,
        }
    }
}

/// An event a transfer is due
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferChange {
    Added,
    Finished,
    Failed,
}

/// The classes of the last transfer list seen
#[derive(Debug, Default)]
pub struct TransferEvents {
    /// `None` until a first list (or a stored map) sets the baseline
    seen: Option<BTreeMap<i64, TransferClass>>,
}

impl TransferEvents {
    /// The tracker from the stored map; `None` or unreadable text means no baseline yet
    pub fn from_stored(stored: Option<&str>) -> TransferEvents {
        let seen = stored
            .and_then(|text| serde_json::from_str::<BTreeMap<i64, String>>(text).ok())
            .map(|map| {
                map.into_iter()
                    .filter_map(|(id, code)| Some((id, TransferClass::from_code(&code)?)))
                    .collect()
            });
        TransferEvents { seen }
    }

    /// The map to store
    pub fn to_stored(&self) -> String {
        let map: BTreeMap<i64, String> = self
            .seen
            .iter()
            .flatten()
            .map(|(id, class)| (*id, class.code().to_string()))
            .collect();
        serde_json::to_string(&map).unwrap_or_else(|_| "{}".to_string())
    }

    /// Take a fresh list of (transfer id, class): the events due, in list order, and
    /// whether the stored map changed. Transfers gone from the list are forgotten.
    pub fn observe(&mut self, list: &[(i64, TransferClass)]) -> (Vec<(i64, TransferChange)>, bool) {
        let latest: BTreeMap<i64, TransferClass> = list.iter().copied().collect();
        let Some(seen) = self.seen.as_ref() else {
            self.seen = Some(latest);
            return (Vec::new(), true);
        };
        let mut changes = Vec::new();
        for (id, class) in list {
            match (seen.get(id), class) {
                (None, class) => {
                    changes.push((*id, TransferChange::Added));
                    match class {
                        TransferClass::Finished => changes.push((*id, TransferChange::Finished)),
                        TransferClass::Failed => changes.push((*id, TransferChange::Failed)),
                        TransferClass::Active => {}
                    }
                }
                (Some(TransferClass::Active), TransferClass::Finished)
                | (Some(TransferClass::Failed), TransferClass::Finished) => {
                    changes.push((*id, TransferChange::Finished))
                }
                (Some(TransferClass::Active), TransferClass::Failed) => {
                    changes.push((*id, TransferChange::Failed))
                }
                _ => {}
            }
        }
        let changed = *seen != latest;
        self.seen = Some(latest);
        (changes, changed)
    }
}

/// The event `origin` of a transfer from its `transfer_origins` label (`watch`, `manual`
/// or `arr:<category>`): an arr category that names Radarr or Sonarr gives that app,
/// another arr category gives `arr`, and a transfer putio-sync did not add gives `putio`
pub fn origin(label: Option<&str>) -> String {
    match label {
        None => "putio".to_string(),
        Some(label) => match label.strip_prefix("arr:") {
            Some(category) => {
                let category = category.to_ascii_lowercase();
                if category.contains("radarr") {
                    "radarr".to_string()
                } else if category.contains("sonarr") {
                    "sonarr".to_string()
                } else {
                    "arr".to_string()
                }
            }
            None => label.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::TransferChange::{Added, Failed, Finished};
    use super::TransferClass::{Active, Failed as Err, Finished as Done};
    use super::*;

    #[test]
    fn statuses_are_classed() {
        assert_eq!(TransferClass::of("COMPLETED"), Done);
        assert_eq!(TransferClass::of("SEEDING"), Done);
        assert_eq!(TransferClass::of("ERROR"), Err);
        assert_eq!(TransferClass::of("DOWNLOADING"), Active);
        assert_eq!(TransferClass::of("IN_QUEUE"), Active);
    }

    #[test]
    fn the_first_list_without_a_stored_map_is_the_baseline() {
        let mut t = TransferEvents::from_stored(None);
        let (changes, changed) = t.observe(&[(1, Active), (2, Done)]);
        assert!(changes.is_empty());
        assert!(changed);
        assert_eq!(t.observe(&[(1, Active), (2, Done)]), (vec![], false));
    }

    #[test]
    fn transitions_send_their_events() {
        let mut t = TransferEvents::from_stored(Some("{}"));
        assert_eq!(t.observe(&[(1, Active)]), (vec![(1, Added)], true));
        assert_eq!(t.observe(&[(1, Done)]), (vec![(1, Finished)], true));
        assert_eq!(
            t.observe(&[(1, Done), (2, Err)]),
            (vec![(2, Added), (2, Failed)], true)
        );
        // A failed transfer that is retried and finishes
        assert_eq!(t.observe(&[(1, Done), (2, Active)]), (vec![], true));
        assert_eq!(
            t.observe(&[(1, Done), (2, Done)]),
            (vec![(2, Finished)], true)
        );
        let mut u = TransferEvents::from_stored(Some(r#"{"3":"x"}"#));
        assert_eq!(u.observe(&[(3, Done)]), (vec![(3, Finished)], true));
        assert_eq!(
            u.observe(&[(4, Done)]),
            (vec![(4, Added), (4, Finished)], true)
        );
        // A finished one never fails or comes back as added
        assert_eq!(u.observe(&[(4, Err)]), (vec![], true));
    }

    #[test]
    fn the_stored_map_round_trips_and_forgets_removed_transfers() {
        let mut t = TransferEvents::from_stored(Some("{}"));
        t.observe(&[(1, Active), (2, Done), (3, Err)]);
        let stored = t.to_stored();
        assert_eq!(stored, r#"{"1":"a","2":"f","3":"x"}"#);
        let mut again = TransferEvents::from_stored(Some(&stored));
        assert_eq!(again.observe(&[(2, Done)]), (vec![], true));
        assert_eq!(again.to_stored(), r#"{"2":"f"}"#);
        assert!(TransferEvents::from_stored(Some("not json")).seen.is_none());
    }

    #[test]
    fn origins() {
        assert_eq!(origin(None), "putio");
        assert_eq!(origin(Some("watch")), "watch");
        assert_eq!(origin(Some("manual")), "manual");
        assert_eq!(origin(Some("arr:radarr")), "radarr");
        assert_eq!(origin(Some("arr:tv-sonarr")), "sonarr");
        assert_eq!(origin(Some("arr:movies/4k")), "arr");
    }
}
