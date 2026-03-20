use std::collections::HashMap;

use anyhow::{anyhow, Result};
use domain::{RunStatus, RunStep, RunStepStatus, WorkflowStep};

pub fn terminal_run_status(run_steps: &[RunStep]) -> Option<RunStatus> {
    if run_steps
        .iter()
        .any(|step| step.status == RunStepStatus::Failed)
    {
        return Some(RunStatus::Failed);
    }

    if run_steps
        .iter()
        .any(|step| step.status == RunStepStatus::Cancelled)
    {
        return Some(RunStatus::Cancelled);
    }

    if !run_steps.is_empty()
        && run_steps
            .iter()
            .all(|step| step.status == RunStepStatus::Completed)
    {
        return Some(RunStatus::Completed);
    }

    None
}

pub fn has_waiting_approval(run_steps: &[RunStep]) -> bool {
    run_steps
        .iter()
        .any(|step| step.status == RunStepStatus::WaitingForApproval)
}

pub fn has_running_step(run_steps: &[RunStep]) -> bool {
    run_steps
        .iter()
        .any(|step| step.status == RunStepStatus::Running)
}

pub fn next_runnable_step(
    run_steps: &[RunStep],
    workflow_steps: &[WorkflowStep],
) -> Result<Option<RunStep>> {
    let workflow_index = workflow_steps
        .iter()
        .map(|step| (step.id, step))
        .collect::<HashMap<_, _>>();

    let run_step_index = run_steps
        .iter()
        .map(|step| (step.workflow_step_id, step))
        .collect::<HashMap<_, _>>();

    let mut ordered = workflow_steps.to_vec();
    ordered.sort_by_key(|step| step.order_index);

    for workflow_step in ordered {
        let Some(run_step) = run_step_index.get(&workflow_step.id) else {
            return Err(anyhow!(
                "missing run step for workflow step {}",
                workflow_step.id
            ));
        };

        if run_step.status != RunStepStatus::Pending {
            continue;
        }

        let dependency_is_satisfied = match workflow_index
            .get(&workflow_step.id)
            .ok_or_else(|| anyhow!("missing workflow step {}", workflow_step.id))?
            .depends_on_step_id
        {
            Some(depends_on_step_id) => run_step_index
                .get(&depends_on_step_id)
                .map(|step| step.status == RunStepStatus::Completed)
                .unwrap_or(false),
            None => true,
        };

        if dependency_is_satisfied {
            return Ok(Some((*run_step).clone()));
        }
    }

    Ok(None)
}
