//! The question the Radarr/Sonarr view asks the processing pipeline (feature 038, R4)
//!
//! `putio-sync-arr` cannot depend on the pipeline crate, so the answer comes through this
//! trait, which the pipeline implements.

/// Whether an item still has a file-changing step to run
pub trait PipelineGate: Send + Sync {
    /// The label of the step that is still due for the item `key` (`transfer:<id>`), such
    /// as `Post-processing: tidy`, or `None` when nothing that changes files is left
    fn pending(&self, key: &str) -> Option<String>;
}

/// The gate for a daemon with no pipeline: nothing is ever pending
#[derive(Debug, Default, Clone, Copy)]
pub struct NoGate;

impl PipelineGate for NoGate {
    fn pending(&self, _key: &str) -> Option<String> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_gate_is_never_pending() {
        assert_eq!(NoGate.pending("transfer:1"), None);
    }
}
