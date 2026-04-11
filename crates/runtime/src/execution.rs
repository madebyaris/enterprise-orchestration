use std::time::Duration;

use domain::{Project, Run, WorkflowStep};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeStepEnvironment {
    pub vars: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetryDecision {
    pub attempt: u32,
    pub should_retry: bool,
    pub backoff_ms: u64,
}

#[derive(Default, Clone)]
pub struct ExecutionRuntime;

impl ExecutionRuntime {
    pub fn assemble_step_environment(
        &self,
        project: &Project,
        run: &Run,
        step: &WorkflowStep,
        extra_env: &[(String, String)],
    ) -> RuntimeStepEnvironment {
        let mut vars = vec![
            ("ORCH_PROJECT_ID".into(), project.id.to_string()),
            ("ORCH_PROJECT_PATH".into(), project.workspace_path.clone()),
            ("ORCH_RUN_ID".into(), run.id.to_string()),
            ("ORCH_WORKFLOW_ID".into(), run.workflow_template_id.to_string()),
            ("ORCH_STEP_EXECUTOR".into(), step.executor_kind.as_str().into()),
        ];
        if let Some(role_id) = step.role_id {
            vars.push(("ORCH_ROLE_ID".into(), role_id.to_string()));
        }
        vars.extend(extra_env.iter().cloned());
        RuntimeStepEnvironment { vars }
    }

    pub fn timeout_for_step(&self, step: &WorkflowStep) -> Duration {
        Duration::from_secs(step.timeout_seconds.unwrap_or(600).max(1) as u64)
    }

    pub fn retry_decision(&self, step: &WorkflowStep, attempt: u32) -> RetryDecision {
        let retry_limit = step.retry_limit.max(0) as u32;
        RetryDecision {
            attempt,
            should_retry: attempt < retry_limit,
            backoff_ms: 1_000 * u64::from(attempt + 1),
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use domain::{ExecutorKind, Project, Run, RunStatus, WorkflowStep};
    use uuid::Uuid;

    use super::ExecutionRuntime;

    #[test]
    fn computes_timeout_and_retry() {
        let runtime = ExecutionRuntime;
        let step = WorkflowStep {
            id: Uuid::new_v4(),
            workflow_template_id: Uuid::new_v4(),
            name: "Step".into(),
            instruction: "Do work".into(),
            order_index: 0,
            executor_kind: ExecutorKind::Shell,
            role_id: None,
            depends_on_step_id: None,
            timeout_seconds: Some(90),
            retry_limit: 2,
            requires_approval: false,
            success_criteria: None,
            artifact_contract: None,
            input_schema: serde_json::json!({}),
            output_schema: serde_json::json!({}),
        };

        assert_eq!(runtime.timeout_for_step(&step).as_secs(), 90);
        assert!(runtime.retry_decision(&step, 1).should_retry);
        assert!(!runtime.retry_decision(&step, 2).should_retry);
    }

    #[test]
    fn assembles_runtime_environment() {
        let runtime = ExecutionRuntime;
        let now = Utc::now();
        let project = Project {
            id: Uuid::new_v4(),
            name: "Project".into(),
            description: None,
            workspace_path: "/workspace".into(),
            repository_url: None,
            default_executor_profile_id: None,
            agents_md_path: None,
            agents_md_updated_at: None,
            archived_at: None,
            created_at: now,
            updated_at: now,
        };
        let run = Run {
            id: Uuid::new_v4(),
            project_id: project.id,
            workflow_template_id: Uuid::new_v4(),
            executor_profile_id: None,
            goal_id: None,
            compiled_by: None,
            assigned_role_id: None,
            effective_executor_kind: None,
            status: RunStatus::Queued,
            requested_by: None,
            created_at: now,
            updated_at: now,
        };
        let step = WorkflowStep {
            id: Uuid::new_v4(),
            workflow_template_id: run.workflow_template_id,
            name: "Step".into(),
            instruction: "Do work".into(),
            order_index: 0,
            executor_kind: ExecutorKind::Codex,
            role_id: Some(Uuid::new_v4()),
            depends_on_step_id: None,
            timeout_seconds: None,
            retry_limit: 0,
            requires_approval: false,
            success_criteria: None,
            artifact_contract: None,
            input_schema: serde_json::json!({}),
            output_schema: serde_json::json!({}),
        };

        let env = runtime.assemble_step_environment(
            &project,
            &run,
            &step,
            &[("EXTRA".into(), "1".into())],
        );

        assert!(env.vars.iter().any(|(key, _)| key == "ORCH_PROJECT_ID"));
        assert!(env.vars.iter().any(|(key, value)| key == "EXTRA" && value == "1"));
    }
}
