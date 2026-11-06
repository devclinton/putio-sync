//! What the API shows of an item (feature 038): the timeline, and the one-line step for
//! its row on the Active page.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::model::{ItemState, Stage, StepState};
use crate::store::{AttemptRow, ItemRow, StepRow};

/// One attempt, as the API shows it
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AttemptView {
    pub n: i64,
    pub result: Option<&'static str>,
    pub exit_code: Option<i32>,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub output: String,
}

/// One step, as the API shows it
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct StepView {
    pub id: i64,
    pub name: String,
    pub kind: &'static str,
    pub required: bool,
    /// The step's state, or `not_reached` for a waiting step of an item that stopped
    pub state: &'static str,
    pub skip_reason: Option<&'static str>,
    pub next_attempt_at: Option<String>,
    pub progress: Option<f64>,
    pub attempts: Vec<AttemptView>,
}

/// One stage of the timeline
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct StageView {
    pub stage: Stage,
    /// The steps in the single line; branch steps are under `branches`
    pub steps: Vec<StepView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branches: Option<BTreeMap<String, Vec<StepView>>>,
    /// `not_configured` when the stage has no steps at all
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skipped: Option<&'static str>,
}

/// `GET /active/{key}/timeline`
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Timeline {
    pub key: String,
    pub name: String,
    pub state: ItemState,
    pub stages: Vec<StageView>,
}

/// The step on an Active row
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct StepSummary {
    pub stage: Stage,
    pub name: String,
    pub state: &'static str,
    pub progress: Option<f64>,
}

fn view(step: &StepRow, attempts: &[AttemptRow], stopped: bool) -> StepView {
    let state = if stopped && step.state == StepState::Waiting {
        "not_reached"
    } else {
        step.state.as_str()
    };
    StepView {
        id: step.id,
        name: step.name.clone(),
        kind: step.kind.as_str(),
        required: step.required,
        state,
        skip_reason: step.skip_reason.map(|r| r.as_str()),
        next_attempt_at: step.next_attempt_at.clone(),
        progress: step.progress,
        attempts: attempts
            .iter()
            .filter(|a| a.step_id == step.id)
            .map(|a| AttemptView {
                n: a.n,
                result: a.result.map(|r| r.as_str()),
                exit_code: a.exit_code,
                started_at: a.started_at.clone(),
                ended_at: a.ended_at.clone(),
                output: a.output.clone(),
            })
            .collect(),
    }
}

/// Build the timeline of `item`
pub fn timeline(item: &ItemRow, steps: &[StepRow], attempts: &[AttemptRow]) -> Timeline {
    // A failed or cancelled item never reaches its later steps
    let stopped = matches!(item.state, ItemState::Failed | ItemState::Cancelled);
    let stages = Stage::ALL
        .into_iter()
        .map(|stage| {
            let in_stage: Vec<&StepRow> = steps.iter().filter(|s| s.stage == stage).collect();
            let mut line = Vec::new();
            let mut branches: BTreeMap<String, Vec<StepView>> = BTreeMap::new();
            for s in &in_stage {
                let v = view(s, attempts, stopped);
                match &s.branch {
                    Some(b) => branches.entry(b.clone()).or_default().push(v),
                    None => line.push(v),
                }
            }
            StageView {
                stage,
                skipped: in_stage.is_empty().then_some("not_configured"),
                branches: (!branches.is_empty()).then_some(branches),
                steps: line,
            }
        })
        .collect();
    Timeline {
        key: item.key.clone(),
        name: item.name.clone(),
        state: item.state,
        stages,
    }
}

/// The step an Active row shows: the running one, the failed one an item waits on, or the
/// next to run. `None` while the item is still downloading, or when it has finished.
pub fn summary(item: &ItemRow, steps: &[StepRow]) -> Option<StepSummary> {
    if item.state.is_terminal() || item.stage == Stage::Downloading {
        return None;
    }
    let pick = steps
        .iter()
        .find(|s| s.state == StepState::Running)
        .or_else(|| {
            steps
                .iter()
                .find(|s| s.state == StepState::Failed && s.required)
        })
        .or_else(|| {
            steps.iter().find(|s| {
                s.stage != Stage::Downloading
                    && matches!(s.state, StepState::Waiting | StepState::Interrupted)
            })
        })?;
    Some(StepSummary {
        stage: pick.stage,
        name: pick.name.clone(),
        state: pick.state.as_str(),
        progress: pick.progress,
    })
}
