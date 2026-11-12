//! Active transfers pipeline metrics (feature 039).

use crate::{global, PipelineAction, PipelineStage, PipelineState, Telemetry};

impl Telemetry {
    /// Set the number of active transfers in `stage` and `state`.
    pub fn set_pipeline_items(&self, stage: PipelineStage, state: PipelineState, n: usize) {
        self.pipeline_items
            .with_label_values(&[stage.as_str(), state.as_str()])
            .set(n as i64);
    }

    /// Count one pause, hold or resume.
    pub fn pipeline_action(&self, action: PipelineAction) {
        self.pipeline_actions
            .with_label_values(&[action.as_str()])
            .inc();
    }

    /// Set whether the Pause local downloads switch is on.
    pub fn set_nas_downloads_paused(&self, paused: bool) {
        self.nas_downloads_paused.set(i64::from(paused));
    }
}

/// Set a pipeline item gauge on the global instance.
pub fn set_items(stage: PipelineStage, state: PipelineState, n: usize) {
    global().set_pipeline_items(stage, state, n);
}

/// Count an action on the global instance.
pub fn action(action: PipelineAction) {
    global().pipeline_action(action);
}

/// Set the switch gauge on the global instance.
pub fn set_nas_paused(paused: bool) {
    global().set_nas_downloads_paused(paused);
}
