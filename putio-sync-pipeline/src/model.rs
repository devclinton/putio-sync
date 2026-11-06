//! The pipeline's vocabulary (feature 038): stages, steps and item states, and how each
//! is written in the database and the API.

use serde::Serialize;

/// Where in its life an item is. The order is the order of the pipeline (FR-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    PreDownload,
    Downloading,
    PreProcessing,
    Subtitles,
    Translating,
    PostProcessing,
    Notifications,
}

impl Stage {
    /// Every stage, in order
    pub const ALL: [Stage; 7] = [
        Stage::PreDownload,
        Stage::Downloading,
        Stage::PreProcessing,
        Stage::Subtitles,
        Stage::Translating,
        Stage::PostProcessing,
        Stage::Notifications,
    ];

    /// The database and API name
    pub fn as_str(self) -> &'static str {
        match self {
            Stage::PreDownload => "pre_download",
            Stage::Downloading => "downloading",
            Stage::PreProcessing => "pre_processing",
            Stage::Subtitles => "subtitles",
            Stage::Translating => "translating",
            Stage::PostProcessing => "post_processing",
            Stage::Notifications => "notifications",
        }
    }

    /// The stage named `s`
    pub fn parse(s: &str) -> Option<Stage> {
        Stage::ALL.into_iter().find(|stage| stage.as_str() == s)
    }

    /// A short label for people
    pub fn label(self) -> &'static str {
        match self {
            Stage::PreDownload => "Pre-download",
            Stage::Downloading => "Downloading",
            Stage::PreProcessing => "Pre-processing",
            Stage::Subtitles => "Subtitles",
            Stage::Translating => "Translating",
            Stage::PostProcessing => "Post-processing",
            Stage::Notifications => "Notifications",
        }
    }
}

impl From<putio_sync_core::hooks::HookStage> for Stage {
    fn from(stage: putio_sync_core::hooks::HookStage) -> Stage {
        use putio_sync_core::hooks::HookStage;
        match stage {
            HookStage::PreDownload => Stage::PreDownload,
            HookStage::PreProcessing => Stage::PreProcessing,
            HookStage::PostProcessing => Stage::PostProcessing,
        }
    }
}

/// What a step does
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StepKind {
    Hook,
    Download,
    Notify,
}

impl StepKind {
    pub fn as_str(self) -> &'static str {
        match self {
            StepKind::Hook => "hook",
            StepKind::Download => "download",
            StepKind::Notify => "notify",
        }
    }

    pub fn parse(s: &str) -> Option<StepKind> {
        match s {
            "hook" => Some(StepKind::Hook),
            "download" => Some(StepKind::Download),
            "notify" => Some(StepKind::Notify),
            _ => None,
        }
    }
}

/// Where one step is
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StepState {
    Waiting,
    Running,
    Done,
    Failed,
    Skipped,
    /// The daemon stopped while it ran; it runs again
    Interrupted,
}

impl StepState {
    pub fn as_str(self) -> &'static str {
        match self {
            StepState::Waiting => "waiting",
            StepState::Running => "running",
            StepState::Done => "done",
            StepState::Failed => "failed",
            StepState::Skipped => "skipped",
            StepState::Interrupted => "interrupted",
        }
    }

    pub fn parse(s: &str) -> Option<StepState> {
        match s {
            "waiting" => Some(StepState::Waiting),
            "running" => Some(StepState::Running),
            "done" => Some(StepState::Done),
            "failed" => Some(StepState::Failed),
            "skipped" => Some(StepState::Skipped),
            "interrupted" => Some(StepState::Interrupted),
            _ => None,
        }
    }

    /// Done or skipped: the step needs nothing more
    pub fn is_settled(self) -> bool {
        matches!(self, StepState::Done | StepState::Skipped)
    }
}

/// Where an item is overall
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemState {
    Running,
    Held,
    WaitingForOperator,
    Done,
    DoneWithWarnings,
    Failed,
    Cancelled,
    Skipped,
}

impl ItemState {
    pub fn as_str(self) -> &'static str {
        match self {
            ItemState::Running => "running",
            ItemState::Held => "held",
            ItemState::WaitingForOperator => "waiting_for_operator",
            ItemState::Done => "done",
            ItemState::DoneWithWarnings => "done_with_warnings",
            ItemState::Failed => "failed",
            ItemState::Cancelled => "cancelled",
            ItemState::Skipped => "skipped",
        }
    }

    pub fn parse(s: &str) -> Option<ItemState> {
        match s {
            "running" => Some(ItemState::Running),
            "held" => Some(ItemState::Held),
            "waiting_for_operator" => Some(ItemState::WaitingForOperator),
            "done" => Some(ItemState::Done),
            "done_with_warnings" => Some(ItemState::DoneWithWarnings),
            "failed" => Some(ItemState::Failed),
            "cancelled" => Some(ItemState::Cancelled),
            "skipped" => Some(ItemState::Skipped),
            _ => None,
        }
    }

    /// Nothing more will happen to the item
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            ItemState::Done
                | ItemState::DoneWithWarnings
                | ItemState::Failed
                | ItemState::Cancelled
                | ItemState::Skipped
        )
    }
}

/// Why a step did not run
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    NotConfigured,
    Category,
    Operator,
    Hook,
}

impl SkipReason {
    pub fn as_str(self) -> &'static str {
        match self {
            SkipReason::NotConfigured => "not_configured",
            SkipReason::Category => "category",
            SkipReason::Operator => "operator",
            SkipReason::Hook => "hook",
        }
    }

    pub fn parse(s: &str) -> Option<SkipReason> {
        match s {
            "not_configured" => Some(SkipReason::NotConfigured),
            "category" => Some(SkipReason::Category),
            "operator" => Some(SkipReason::Operator),
            "hook" => Some(SkipReason::Hook),
            _ => None,
        }
    }
}

/// How an attempt ended
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AttemptResult {
    Done,
    Failed,
    Timeout,
    Skip,
    Interrupted,
}

impl AttemptResult {
    pub fn as_str(self) -> &'static str {
        match self {
            AttemptResult::Done => "done",
            AttemptResult::Failed => "failed",
            AttemptResult::Timeout => "timeout",
            AttemptResult::Skip => "skip",
            AttemptResult::Interrupted => "interrupted",
        }
    }

    pub fn parse(s: &str) -> Option<AttemptResult> {
        match s {
            "done" => Some(AttemptResult::Done),
            "failed" => Some(AttemptResult::Failed),
            "timeout" => Some(AttemptResult::Timeout),
            "skip" => Some(AttemptResult::Skip),
            "interrupted" => Some(AttemptResult::Interrupted),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stages_are_in_order_and_round_trip() {
        let mut sorted = Stage::ALL;
        sorted.sort();
        assert_eq!(sorted, Stage::ALL);
        for stage in Stage::ALL {
            assert_eq!(Stage::parse(stage.as_str()), Some(stage));
        }
        assert_eq!(Stage::parse("nope"), None);
        assert_eq!(Stage::ALL[0].as_str(), "pre_download");
        assert_eq!(Stage::ALL[6].as_str(), "notifications");
    }

    #[test]
    fn every_name_round_trips() {
        for s in [StepKind::Hook, StepKind::Download, StepKind::Notify] {
            assert_eq!(StepKind::parse(s.as_str()), Some(s));
        }
        for s in [
            StepState::Waiting,
            StepState::Running,
            StepState::Done,
            StepState::Failed,
            StepState::Skipped,
            StepState::Interrupted,
        ] {
            assert_eq!(StepState::parse(s.as_str()), Some(s));
        }
        for s in [
            ItemState::Running,
            ItemState::Held,
            ItemState::WaitingForOperator,
            ItemState::Done,
            ItemState::DoneWithWarnings,
            ItemState::Failed,
            ItemState::Cancelled,
            ItemState::Skipped,
        ] {
            assert_eq!(ItemState::parse(s.as_str()), Some(s));
        }
        for s in [
            SkipReason::NotConfigured,
            SkipReason::Category,
            SkipReason::Operator,
            SkipReason::Hook,
        ] {
            assert_eq!(SkipReason::parse(s.as_str()), Some(s));
        }
        for s in [
            AttemptResult::Done,
            AttemptResult::Failed,
            AttemptResult::Timeout,
            AttemptResult::Skip,
            AttemptResult::Interrupted,
        ] {
            assert_eq!(AttemptResult::parse(s.as_str()), Some(s));
        }
    }

    #[test]
    fn terminal_states() {
        assert!(ItemState::Done.is_terminal());
        assert!(ItemState::Skipped.is_terminal());
        assert!(!ItemState::Held.is_terminal());
        assert!(!ItemState::WaitingForOperator.is_terminal());
    }
}
