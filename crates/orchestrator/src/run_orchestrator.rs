use anyhow::{anyhow, Result};
use domain::{
    ApprovalGate, EventEnvelope, EventScope, NewRun, Run, RunStatus, RunStep, RunStepStatus,
    WorkflowStep,
};
use observability::EventBus;
use persistence::OrchestratorStore;
use serde_json::json;
use uuid::Uuid;

use crate::{
    approval_service::ApprovalService,
    step_dispatcher::{
        has_running_step, has_waiting_approval, next_runnable_step, terminal_run_status,
    },
};

#[derive(Debug, Clone)]
pub struct RunStateSnapshot {
    pub run: Run,
    pub run_steps: Vec<RunStep>,
    pub workflow_steps: Vec<WorkflowStep>,
    pub pending_approval: Option<ApprovalGate>,
}

#[derive(Clone)]
pub struct RunOrchestrator {
    store: OrchestratorStore,
    events: EventBus,
    approvals: ApprovalService,
}

impl RunOrchestrator {
    pub fn new(store: OrchestratorStore, events: EventBus) -> Self {
        let approvals = ApprovalService::new(store.clone(), events.clone());
        Self {
            store,
            events,
            approvals,
        }
    }

    pub fn start_run(&self, input: NewRun) -> Result<RunStateSnapshot> {
        let run = self.store.create_run(input)?;
        self.emit(EventEnvelope::new(
            EventScope::Run,
            "run.created",
            "Run created and queued",
            json!({
                "run_id": run.id,
                "workflow_template_id": run.workflow_template_id,
            }),
        ))?;

        self.drive_run(run.id)
    }

    pub fn complete_running_step(&self, run_step_id: Uuid) -> Result<RunStateSnapshot> {
        let step = self
            .store
            .get_run_step(run_step_id)?
            .ok_or_else(|| anyhow!("run step {run_step_id} not found"))?;

        self.store
            .update_run_step_status(run_step_id, RunStepStatus::Completed, None)?;
        self.emit(EventEnvelope::new(
            EventScope::Run,
            "run_step.completed",
            "Workflow step completed",
            json!({
                "run_id": step.run_id,
                "run_step_id": run_step_id,
            }),
        ))?;

        self.drive_run(step.run_id)
    }

    pub fn fail_running_step(
        &self,
        run_step_id: Uuid,
        reason: impl Into<String>,
    ) -> Result<RunStateSnapshot> {
        let step = self
            .store
            .get_run_step(run_step_id)?
            .ok_or_else(|| anyhow!("run step {run_step_id} not found"))?;
        let reason = reason.into();

        self.store
            .update_run_step_status(run_step_id, RunStepStatus::Failed, None)?;
        self.store
            .update_run_status(step.run_id, RunStatus::Failed)?;
        self.emit(EventEnvelope::new(
            EventScope::Run,
            "run_step.failed",
            "Workflow step failed",
            json!({
                "run_id": step.run_id,
                "run_step_id": run_step_id,
                "reason": reason,
            }),
        ))?;

        self.snapshot(step.run_id)
    }

    pub fn approve_gate(
        &self,
        approval_id: Uuid,
        resolved_by: Option<String>,
        notes: Option<String>,
    ) -> Result<RunStateSnapshot> {
        let gate = self.approvals.approve(approval_id, resolved_by, notes)?;
        if let Some(run_step_id) = gate.run_step_id {
            self.store
                .update_run_step_status(run_step_id, RunStepStatus::Running, None)?;
            self.store
                .update_run_status(gate.run_id, RunStatus::Running)?;
            self.emit(EventEnvelope::new(
                EventScope::Run,
                "run_step.started",
                "Workflow step started after approval",
                json!({
                    "run_id": gate.run_id,
                    "run_step_id": run_step_id,
                }),
            ))?;
            return self.snapshot(gate.run_id);
        }

        self.drive_run(gate.run_id)
    }

    pub fn reject_gate(
        &self,
        approval_id: Uuid,
        resolved_by: Option<String>,
        notes: Option<String>,
    ) -> Result<RunStateSnapshot> {
        let gate = self.approvals.reject(approval_id, resolved_by, notes)?;
        if let Some(run_step_id) = gate.run_step_id {
            self.store
                .update_run_step_status(run_step_id, RunStepStatus::Cancelled, None)?;
        }
        self.store
            .update_run_status(gate.run_id, RunStatus::Cancelled)?;
        self.emit(EventEnvelope::new(
            EventScope::Run,
            "run.cancelled",
            "Run cancelled after approval rejection",
            json!({
                "run_id": gate.run_id,
                "approval_id": gate.id,
            }),
        ))?;

        self.snapshot(gate.run_id)
    }

    pub fn recover_in_progress_runs(&self) -> Result<Vec<RunStateSnapshot>> {
        let runs = self.store.list_runs()?;
        runs.into_iter()
            .filter(|run| {
                matches!(
                    run.status,
                    RunStatus::Queued | RunStatus::Running | RunStatus::WaitingForApproval
                )
            })
            .map(|run| self.drive_run(run.id))
            .collect()
    }

    pub fn drive_run(&self, run_id: Uuid) -> Result<RunStateSnapshot> {
        let snapshot = self.snapshot(run_id)?;

        if let Some(status) = terminal_run_status(&snapshot.run_steps) {
            self.store.update_run_status(run_id, status.clone())?;
            self.emit(EventEnvelope::new(
                EventScope::Run,
                format!("run.{}", status.as_str()),
                format!("Run transitioned to {}", status.as_str()),
                json!({"run_id": run_id, "status": status.as_str()}),
            ))?;
            return self.snapshot(run_id);
        }

        if has_waiting_approval(&snapshot.run_steps) {
            self.store
                .update_run_status(run_id, RunStatus::WaitingForApproval)?;
            return self.snapshot(run_id);
        }

        if has_running_step(&snapshot.run_steps) {
            self.store.update_run_status(run_id, RunStatus::Running)?;
            return self.snapshot(run_id);
        }

        if let Some(next_step) = next_runnable_step(&snapshot.run_steps, &snapshot.workflow_steps)?
        {
            let workflow_step = snapshot
                .workflow_steps
                .iter()
                .find(|step| step.id == next_step.workflow_step_id)
                .ok_or_else(|| anyhow!("missing workflow step for run step {}", next_step.id))?;

            if workflow_step.requires_approval {
                self.store.update_run_step_status(
                    next_step.id,
                    RunStepStatus::WaitingForApproval,
                    None,
                )?;
                self.store
                    .update_run_status(run_id, RunStatus::WaitingForApproval)?;

                if self
                    .store
                    .find_pending_approval_for_run_step(next_step.id)?
                    .is_none()
                {
                    self.approvals
                        .request_for_step(run_id, next_step.id, Some("system".into()))?;
                }
            } else {
                self.store
                    .update_run_step_status(next_step.id, RunStepStatus::Running, None)?;
                self.store.update_run_status(run_id, RunStatus::Running)?;
                self.emit(EventEnvelope::new(
                    EventScope::Run,
                    "run_step.started",
                    "Workflow step started",
                    json!({
                        "run_id": run_id,
                        "run_step_id": next_step.id,
                        "workflow_step_id": next_step.workflow_step_id,
                    }),
                ))?;
            }
        } else {
            self.store.update_run_status(run_id, RunStatus::Queued)?;
        }

        self.snapshot(run_id)
    }

    pub fn snapshot(&self, run_id: Uuid) -> Result<RunStateSnapshot> {
        let run = self
            .store
            .get_run(run_id)?
            .ok_or_else(|| anyhow!("run {run_id} not found"))?;
        let run_steps = self.store.list_run_steps(run_id)?;
        let workflow_steps = self.store.list_workflow_steps(run.workflow_template_id)?;
        let mut pending_approval = None;
        for step in &run_steps {
            if let Some(gate) = self.store.find_pending_approval_for_run_step(step.id)? {
                pending_approval = Some(gate);
                break;
            }
        }

        Ok(RunStateSnapshot {
            run,
            run_steps,
            workflow_steps,
            pending_approval,
        })
    }

    fn emit(&self, event: EventEnvelope) -> Result<()> {
        self.store.record_event(&event)?;
        self.events.publish(event);
        Ok(())
    }
}
