//! When each *arr app last reached the RPC, keyed by the first `User-Agent` token
//! (`Sonarr/4.0.9` → `Sonarr`), for the Settings card

use std::collections::HashMap;
use std::sync::Mutex;

use chrono::{DateTime, Utc};

/// Bounds the map, since the agent name comes from the client
const MAX_AGENTS: usize = 16;
const MAX_AGENT_LEN: usize = 32;

/// Last authenticated contact per agent
#[derive(Debug, Default)]
pub struct Contacts {
    seen: Mutex<HashMap<String, DateTime<Utc>>>,
}

impl Contacts {
    /// Record a contact from `user_agent` now
    pub fn record(&self, user_agent: Option<&str>) {
        self.record_at(user_agent, Utc::now());
    }

    fn record_at(&self, user_agent: Option<&str>, at: DateTime<Utc>) {
        let agent = agent_name(user_agent);
        let mut seen = self.seen.lock().unwrap_or_else(|e| e.into_inner());
        if seen.len() >= MAX_AGENTS && !seen.contains_key(&agent) {
            // Drop the oldest so a misbehaving client can't grow the map
            if let Some(oldest) = seen.iter().min_by_key(|(_, t)| **t).map(|(k, _)| k.clone()) {
                seen.remove(&oldest);
            }
        }
        seen.insert(agent, at);
    }

    /// Every agent and its last contact, most recent first
    pub fn snapshot(&self) -> Vec<(String, DateTime<Utc>)> {
        let seen = self.seen.lock().unwrap_or_else(|e| e.into_inner());
        let mut list: Vec<_> = seen.iter().map(|(k, v)| (k.clone(), *v)).collect();
        list.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        list
    }
}

/// The product name of a `User-Agent`, limited to safe characters
fn agent_name(user_agent: Option<&str>) -> String {
    let token = user_agent
        .and_then(|ua| ua.split_whitespace().next())
        .and_then(|t| t.split('/').next())
        .unwrap_or("");
    let name: String = token
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        .take(MAX_AGENT_LEN)
        .collect();
    if name.is_empty() {
        "unknown".to_string()
    } else {
        name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn names_from_user_agents() {
        assert_eq!(
            agent_name(Some("Sonarr/4.0.9.2244 (ubuntu 22.04)")),
            "Sonarr"
        );
        assert_eq!(agent_name(Some("Radarr/5.2")), "Radarr");
        assert_eq!(agent_name(Some("<script>/1")), "script");
        assert_eq!(agent_name(None), "unknown");
        assert_eq!(agent_name(Some("   ")), "unknown");
    }

    #[test]
    fn keeps_latest_and_bounds_size() {
        let contacts = Contacts::default();
        let t0 = Utc::now();
        contacts.record_at(Some("Sonarr/4"), t0);
        contacts.record_at(Some("Radarr/5"), t0 + Duration::seconds(1));
        contacts.record_at(Some("Sonarr/4"), t0 + Duration::seconds(2));
        let snap = contacts.snapshot();
        assert_eq!(snap[0], ("Sonarr".to_string(), t0 + Duration::seconds(2)));
        assert_eq!(snap[1].0, "Radarr");

        for i in 0..40 {
            contacts.record_at(Some(&format!("agent{i}/1")), t0 + Duration::seconds(10 + i));
        }
        assert_eq!(contacts.snapshot().len(), MAX_AGENTS);
    }
}
