//! The arr gate (feature 038, R7): while an item still has processing to do, Radarr and
//! Sonarr see it as not quite finished, so they never import a half-processed folder.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use putio_sync_core::pipeline_gate::PipelineGate;

/// Answers [`PipelineGate::pending`] from a map the service refreshes whenever a step or
/// item changes, so an arr request never waits on the database
#[derive(Clone, Default)]
pub struct Gate {
    pending: Arc<RwLock<HashMap<String, String>>>,
}

impl Gate {
    /// Replace what is pending: item key to a label such as `post_processing: tidy`
    pub fn set(&self, pending: HashMap<String, String>) {
        if let Ok(mut map) = self.pending.write() {
            *map = pending;
        }
    }
}

impl PipelineGate for Gate {
    fn pending(&self, key: &str) -> Option<String> {
        self.pending.read().ok()?.get(key).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_listed_keys_are_pending() {
        let gate = Gate::default();
        assert_eq!(gate.pending("transfer:1"), None);
        gate.set(HashMap::from([(
            "transfer:1".to_string(),
            "post_processing: tidy".to_string(),
        )]));
        assert_eq!(
            gate.pending("transfer:1").as_deref(),
            Some("post_processing: tidy")
        );
        assert_eq!(gate.pending("transfer:2"), None);
        gate.set(HashMap::new());
        assert_eq!(gate.pending("transfer:1"), None);
    }
}
