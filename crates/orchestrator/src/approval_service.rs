use anyhow::Result;
use domain::{ApprovalDecision, ApprovalGate, EventEnvelope, EventScope};
use observability::EventBus;
use persistence::OrchestratorStore;
use serde_json::json;
use uuid::Uuid;

#[derive(Clone)]
pub struct ApprovalService {
    store: OrchestratorStore,
    events: EventBus,
}

impl ApprovalService {
    pub fn new(store: OrchestratorStore, events: EventBus) -> Self {
        Self { store, events }
    }

    pub fn request_for_step(
        &self,
        run_id: Uuid,
        run_step_id: Uuid,
        requested_by: Option<String>,
    ) -> Result<ApprovalGate> {
        let gate = self
            .store
            .create_approval_gate(run_id, Some(run_step_id), requested_by)?;

        self.emit(EventEnvelope::new(
            EventScope::Approval,
            "approval.requested",
            "Approval requested for workflow step",
            json!({
                "approval_id": gate.id,
                "run_id": gate.run_id,
                "run_step_id": gate.run_step_id,
            }),
        ))?;

        Ok(gate)
    }

    pub fn approve(
        &self,
        approval_id: Uuid,
        resolved_by: Option<String>,
        notes: Option<String>,
    ) -> Result<ApprovalGate> {
        let gate = self.store.update_approval_gate(
            approval_id,
            ApprovalDecision::Approved,
            resolved_by,
            notes,
        )?;

        self.emit(EventEnvelope::new(
            EventScope::Approval,
            "approval.approved",
            "Approval gate approved",
            json!({
                "approval_id": gate.id,
                "run_id": gate.run_id,
                "run_step_id": gate.run_step_id,
            }),
        ))?;

        Ok(gate)
    }

    pub fn reject(
        &self,
        approval_id: Uuid,
        resolved_by: Option<String>,
        notes: Option<String>,
    ) -> Result<ApprovalGate> {
        let gate = self.store.update_approval_gate(
            approval_id,
            ApprovalDecision::Rejected,
            resolved_by,
            notes,
        )?;

        self.emit(EventEnvelope::new(
            EventScope::Approval,
            "approval.rejected",
            "Approval gate rejected",
            json!({
                "approval_id": gate.id,
                "run_id": gate.run_id,
                "run_step_id": gate.run_step_id,
            }),
        ))?;

        Ok(gate)
    }

    fn emit(&self, event: EventEnvelope) -> Result<()> {
        self.store.record_event(&event)?;
        self.events.publish(event);
        Ok(())
    }
}
