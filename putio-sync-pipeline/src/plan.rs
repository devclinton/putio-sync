//! The planner (feature 038): the steps an item will pass, from the configured hooks.
//!
//! Stages with nothing to do get no rows; the timeline shows them as not configured. The
//! plan is fixed when the item is first seen, so editing hooks later never reshuffles an
//! item that is half through.

use putio_sync_core::hooks::HookConfig;

use crate::model::{SkipReason, Stage, StepKind, StepState};

/// One step an item will pass
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedStep {
    pub stage: Stage,
    /// A parallel branch, such as a language code; `None` for the usual single line
    pub branch: Option<String>,
    /// The order inside the stage
    pub position: i64,
    pub kind: StepKind,
    /// The hook's name, or `download` / `notify`
    pub name: String,
    pub required: bool,
    pub changes_files: bool,
    /// `Waiting`, or `Skipped` with a reason
    pub state: StepState,
    pub skip_reason: Option<SkipReason>,
}

/// What the planner needs to know about an item
#[derive(Debug, Clone, Copy)]
pub struct PlanInput<'a> {
    pub category: Option<&'a str>,
    /// Whether the item has a Put.io transfer to hold back (pre-download hooks need one)
    pub has_transfer: bool,
    /// Whether the files are already on the NAS: the download step is then done
    pub downloaded: bool,
}

/// The ordered steps of an item
pub fn plan(input: PlanInput<'_>, hooks: &[HookConfig]) -> Vec<PlannedStep> {
    let mut steps = Vec::new();
    for stage in Stage::ALL {
        match stage {
            Stage::Downloading => steps.push(PlannedStep {
                stage,
                branch: None,
                position: 0,
                kind: StepKind::Download,
                name: "download".to_string(),
                required: true,
                changes_files: false,
                state: if input.downloaded {
                    StepState::Done
                } else {
                    StepState::Waiting
                },
                skip_reason: None,
            }),
            Stage::Notifications => steps.push(PlannedStep {
                stage,
                branch: None,
                position: 0,
                kind: StepKind::Notify,
                name: "notify".to_string(),
                required: false,
                changes_files: false,
                state: StepState::Waiting,
                skip_reason: None,
            }),
            _ => {
                let in_stage = hooks
                    .iter()
                    .filter(|h| h.enabled && h.stage.map(Stage::from) == Some(stage));
                for (position, hook) in in_stage.enumerate() {
                    // An unknown category is judged when the step is about to run
                    let wrong_category = !hook.categories.is_empty()
                        && input.category.is_some_and(|c| {
                            !hook.categories.iter().any(|h| h.eq_ignore_ascii_case(c))
                        });
                    let no_gate = stage == Stage::PreDownload && !input.has_transfer;
                    let skip = if wrong_category {
                        Some(SkipReason::Category)
                    } else if no_gate {
                        Some(SkipReason::NotConfigured)
                    } else {
                        None
                    };
                    steps.push(PlannedStep {
                        stage,
                        branch: None,
                        position: position as i64,
                        kind: StepKind::Hook,
                        name: hook.name.clone(),
                        required: hook.required,
                        changes_files: true,
                        state: if skip.is_some() {
                            StepState::Skipped
                        } else {
                            StepState::Waiting
                        },
                        skip_reason: skip,
                    });
                }
            }
        }
    }
    steps
}

/// Whether any enabled hook changes files, which is what turns staging on in `auto`
pub fn changes_files(hooks: &[HookConfig]) -> bool {
    hooks.iter().any(|h| h.enabled && h.stage.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use putio_sync_core::hooks::HookStage;

    fn hook(name: &str, stage: HookStage) -> HookConfig {
        HookConfig {
            name: name.to_string(),
            stage: Some(stage),
            command: Some(vec!["/bin/true".to_string()]),
            ..Default::default()
        }
    }

    fn input(category: Option<&str>) -> PlanInput<'_> {
        PlanInput {
            category,
            has_transfer: true,
            downloaded: false,
        }
    }

    #[test]
    fn no_hooks_gives_download_and_notify_only() {
        let steps = plan(input(None), &[]);
        let kinds: Vec<_> = steps.iter().map(|s| (s.stage, s.kind)).collect();
        assert_eq!(
            kinds,
            vec![
                (Stage::Downloading, StepKind::Download),
                (Stage::Notifications, StepKind::Notify)
            ]
        );
        assert!(!steps[1].changes_files, "notify changes nothing");
    }

    #[test]
    fn hooks_keep_config_order_inside_their_stage() {
        let hooks = vec![
            hook("b", HookStage::PostProcessing),
            hook("a", HookStage::PreProcessing),
            hook("c", HookStage::PostProcessing),
        ];
        let steps = plan(input(None), &hooks);
        let order: Vec<_> = steps.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(order, vec!["download", "a", "b", "c", "notify"]);
        assert_eq!(steps[2].position, 0);
        assert_eq!(steps[3].position, 1);
        assert!(steps[1].changes_files);
    }

    #[test]
    fn category_filters_skip_with_a_reason() {
        let mut h = hook("tidy", HookStage::PostProcessing);
        h.categories = vec!["Sonarr".to_string()];
        let tv = plan(input(Some("sonarr")), &[h.clone()]);
        assert_eq!(tv[1].state, StepState::Waiting, "case does not matter");
        let movies = plan(input(Some("radarr")), &[h.clone()]);
        assert_eq!(movies[1].state, StepState::Skipped);
        assert_eq!(movies[1].skip_reason, Some(SkipReason::Category));
        // No category yet: left for the step to judge when it runs
        let none = plan(input(None), &[h]);
        assert_eq!(none[1].state, StepState::Waiting);
    }

    #[test]
    fn disabled_hooks_are_left_out_and_pre_download_needs_a_transfer() {
        let mut off = hook("off", HookStage::PreProcessing);
        off.enabled = false;
        let gate = hook("gate", HookStage::PreDownload);
        let steps = plan(
            PlanInput {
                category: None,
                has_transfer: false,
                downloaded: true,
            },
            &[off, gate],
        );
        assert!(steps.iter().all(|s| s.name != "off"));
        let gate = steps.iter().find(|s| s.name == "gate").unwrap();
        assert_eq!(gate.skip_reason, Some(SkipReason::NotConfigured));
        assert_eq!(
            steps
                .iter()
                .find(|s| s.kind == StepKind::Download)
                .unwrap()
                .state,
            StepState::Done
        );
    }

    #[test]
    fn any_stage_hook_turns_staging_on() {
        assert!(!changes_files(&[]));
        assert!(changes_files(&[hook("x", HookStage::PostProcessing)]));
        let event_hook = HookConfig {
            events: vec!["*".to_string()],
            ..Default::default()
        };
        assert!(!changes_files(&[event_hook]));
    }
}
