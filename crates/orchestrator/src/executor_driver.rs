use std::sync::Arc;

use anyhow::Result;
use domain::{EventEnvelope, ExecutorKind, RunStepStatus};

use crate::{RunOrchestrator, RunStateSnapshot};
use executors::{
    ClaudeCodeAdapter, CodexAdapter, ExecutorAdapter, ExecutorRunRequest, NativeCliAiAdapter,
    OpenCodeAdapter, ShellExecutorAdapter,
};
use observability::EventBus;
use parking_lot::Mutex;
use persistence::OrchestratorStore;
use tokio::task;
use uuid::Uuid;

pub struct ExecutorDriver {
    store: OrchestratorStore,
    events: EventBus,
    adapters: std::collections::HashMap<ExecutorKind, Arc<dyn ExecutorAdapter + Send + Sync>>,
    active_steps: Arc<Mutex<std::collections::HashSet<Uuid>>>,
}

impl ExecutorDriver {
    pub fn new(store: OrchestratorStore, events: EventBus) -> Self {
        let mut adapters: std::collections::HashMap<
            ExecutorKind,
            Arc<dyn ExecutorAdapter + Send + Sync>,
        > = std::collections::HashMap::new();

        adapters.insert(
            ExecutorKind::Shell,
            Arc::new(ShellExecutorAdapter) as Arc<dyn ExecutorAdapter + Send + Sync>,
        );
        adapters.insert(
            ExecutorKind::NativeCliAi,
            Arc::new(NativeCliAiAdapter::default()) as Arc<dyn ExecutorAdapter + Send + Sync>,
        );
        adapters.insert(
            ExecutorKind::ClaudeCode,
            Arc::new(ClaudeCodeAdapter) as Arc<dyn ExecutorAdapter + Send + Sync>,
        );
        adapters.insert(
            ExecutorKind::Codex,
            Arc::new(CodexAdapter) as Arc<dyn ExecutorAdapter + Send + Sync>,
        );
        adapters.insert(
            ExecutorKind::OpenCode,
            Arc::new(OpenCodeAdapter) as Arc<dyn ExecutorAdapter + Send + Sync>,
        );

        Self {
            store,
            events,
            adapters,
            active_steps: Arc::new(Mutex::new(std::collections::HashSet::new())),
        }
    }

    pub fn with_adapter(
        mut self,
        kind: ExecutorKind,
        adapter: impl ExecutorAdapter + Send + Sync + 'static,
    ) -> Self {
        self.adapters
            .insert(kind, Arc::new(adapter) as Arc<dyn ExecutorAdapter + Send + Sync>);
        self
    }

    pub fn is_step_active(&self, step_id: Uuid) -> bool {
        self.active_steps.lock().contains(&step_id)
    }

    pub async fn drive_step(
        &self,
        step_id: Uuid,
        orchestrator: &RunOrchestrator,
    ) -> Result<RunStateSnapshot> {
        {
            let mut active = self.active_steps.lock();
            if active.contains(&step_id) {
                anyhow::bail!("step {step_id} is already being driven");
            }
            active.insert(step_id);
        }

        let result = self.execute_step(step_id, orchestrator).await;

        {
            let mut active = self.active_steps.lock();
            active.remove(&step_id);
        }

        result
    }

    async fn execute_step(
        &self,
        step_id: Uuid,
        orchestrator: &RunOrchestrator,
    ) -> Result<RunStateSnapshot> {
        let step = self
            .store
            .get_run_step(step_id)?
            .ok_or_else(|| anyhow::anyhow!("run step {step_id} not found"))?;

        if step.status != RunStepStatus::Running {
            return orchestrator.snapshot(step.run_id);
        }

        let run = self
            .store
            .get_run(step.run_id)?
            .ok_or_else(|| anyhow::anyhow!("run {} not found", step.run_id))?;

        let workflow_step = self
            .store
            .get_workflow_step(step.workflow_step_id)?
            .ok_or_else(|| {
                anyhow::anyhow!("workflow step {} not found", step.workflow_step_id)
            })?;

        let step_executor_kind = workflow_step.executor_kind.clone();
        let executor_profile = match run.executor_profile_id {
            Some(id) => self.store.get_executor_profile(id)?,
            None => None,
        };

        let adapter = self
            .adapters
            .get(&step_executor_kind)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("no adapter for {:?}", step_executor_kind))?;

        let project = self
            .store
            .get_project(run.project_id)?
            .ok_or_else(|| anyhow::anyhow!("project {} not found", run.project_id))?;

        let project = project.clone();

        let request = ExecutorRunRequest {
            prompt: workflow_step.instruction.clone(),
            workspace_path: Some(project.workspace_path.clone()),
            permission_mode: None,
            binary_path: executor_profile
                .as_ref()
                .filter(|profile| profile.kind == step_executor_kind)
                .and_then(|profile| profile.binary_path.clone()),
            config_json: executor_profile
                .as_ref()
                .filter(|profile| profile.kind == step_executor_kind)
                .map(|profile| profile.config_json.clone())
                .unwrap_or_else(|| serde_json::json!({})),
            orchestration_env: vec![
                ("ORCH_RUN_ID".to_string(), run.id.to_string()),
                ("ORCH_STEP_ID".to_string(), step.id.to_string()),
                (
                    "ORCH_EXECUTOR_KIND".to_string(),
                    step_executor_kind.as_str().to_string(),
                ),
            ],
        };

        let adapter_available = request
            .binary_path
            .as_ref()
            .map(|path| std::path::Path::new(path).exists())
            .unwrap_or_else(|| adapter.detect().available);
        if !adapter_available {
            return orchestrator.fail_running_step(
                step_id,
                format!("{} is not available on PATH", adapter.display_name()),
            );
        }

        let store = self.store.clone();
        let events = self.events.clone();

        let session_result = task::spawn_blocking(move || {
            let mut on_event = |event: EventEnvelope| {
                let store = store.clone();
                let events = events.clone();
                if let Err(e) = store.record_event(&event) {
                    tracing::error!("failed to record event: {}", e);
                }
                events.publish(event);
            };

            adapter.start_run(&request, &mut on_event)
        })
        .await?;

        let session = match session_result {
            Ok(session) => session,
            Err(error) => {
                return orchestrator.fail_running_step(step_id, error.to_string());
            }
        };

        self.store
            .update_run_step_external_session(step_id, session.session_id)?;

        orchestrator.complete_running_step(step_id)
    }

    pub async fn poll_and_drive_ready_steps(
        &self,
        orchestrator: &RunOrchestrator,
    ) -> Result<Vec<RunStateSnapshot>> {
        let runs = self.store.list_runs()?;
        let mut snapshots = Vec::new();

        for run in runs {
            if run.status != domain::RunStatus::Running {
                continue;
            }

            let steps = self.store.list_run_steps(run.id)?;
            for step in steps {
                if step.status == RunStepStatus::Running && !self.is_step_active(step.id) {
                    match self.drive_step(step.id, orchestrator).await {
                        Ok(snapshot) => snapshots.push(snapshot),
                        Err(e) => {
                            tracing::error!("failed to drive step {}: {}", step.id, e);
                            let _ = orchestrator.fail_running_step(step.id, e.to_string());
                        }
                    }
                }
            }
        }

        Ok(snapshots)
    }
}
