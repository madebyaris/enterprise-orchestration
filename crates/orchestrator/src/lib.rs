pub mod approval_service;
pub mod executor_driver;
pub mod prompt_builder;
pub mod retry_policy;
pub mod run_orchestrator;
pub mod step_dispatcher;
pub mod super_owner;
pub mod workflow_compiler;

pub use approval_service::ApprovalService;
pub use executor_driver::ExecutorDriver;
pub use retry_policy::RetryPolicy;
pub use run_orchestrator::{RunOrchestrator, RunStateSnapshot};
pub use super_owner::{CompiledGoal, SuperOwner};

#[cfg(test)]
mod tests {
    use domain::{
        ExecutorKind, NewRun, NewWorkflowStep, NewWorkflowTemplate, RunStatus, RunStepStatus,
    };
    use observability::EventBus;
    use persistence::OrchestratorStore;

    use crate::{RunOrchestrator, SuperOwner};

    fn setup_store() -> (
        OrchestratorStore,
        RunOrchestrator,
        domain::Project,
        domain::ExecutorProfile,
    ) {
        let store = OrchestratorStore::open_in_memory().expect("store");
        let events = EventBus::default();
        let orchestrator = RunOrchestrator::new(store.clone(), events);
        let project = store
            .create_project(domain::NewProject {
                name: "Enterprise Orchestration".into(),
                description: None,
                workspace_path: "/workspace".into(),
                repository_url: None,
                default_executor_profile_id: None,
            })
            .expect("project");
        let executor = store
            .create_executor_profile(domain::NewExecutorProfile {
                name: "nca".into(),
                kind: ExecutorKind::NativeCliAi,
                binary_path: Some("nca".into()),
                config_json: serde_json::json!({}),
            })
            .expect("executor");

        (store, orchestrator, project, executor)
    }

    #[test]
    fn sequential_workflow_executes_in_order() {
        let (store, orchestrator, project, executor) = setup_store();
        let workflow = store
            .create_workflow(NewWorkflowTemplate {
                project_id: Some(project.id),
                name: "Sequential flow".into(),
                description: None,
                steps: vec![
                    NewWorkflowStep {
                        name: "Step 1".into(),
                        instruction: "First".into(),
                        order_index: 0,
                        executor_kind: ExecutorKind::Shell,
                        role_id: None,
                        depends_on_step_id: None,
                        timeout_seconds: None,
                        retry_limit: 0,
                        requires_approval: false,
                        success_criteria: None,
                        artifact_contract: None,
                        input_schema: serde_json::json!({}),
                        output_schema: serde_json::json!({}),
                    },
                    NewWorkflowStep {
                        name: "Step 2".into(),
                        instruction: "Second".into(),
                        order_index: 1,
                        executor_kind: ExecutorKind::Shell,
                        role_id: None,
                        depends_on_step_id: None,
                        timeout_seconds: None,
                        retry_limit: 0,
                        requires_approval: false,
                        success_criteria: None,
                        artifact_contract: None,
                        input_schema: serde_json::json!({}),
                        output_schema: serde_json::json!({}),
                    },
                ],
            })
            .expect("workflow");

        let started = orchestrator
            .start_run(NewRun {
                project_id: project.id,
                workflow_template_id: workflow.id,
                executor_profile_id: Some(executor.id),
                goal_id: None,
                compiled_by: None,
                assigned_role_id: None,
                effective_executor_kind: None,
                requested_by: Some("operator".into()),
            })
            .expect("run start");

        assert_eq!(started.run.status, RunStatus::Running);
        assert_eq!(started.run_steps[0].status, RunStepStatus::Running);
        assert_eq!(started.run_steps[1].status, RunStepStatus::Pending);

        let after_first = orchestrator
            .complete_running_step(started.run_steps[0].id)
            .expect("complete first");
        assert_eq!(after_first.run.status, RunStatus::Running);
        assert_eq!(after_first.run_steps[0].status, RunStepStatus::Completed);
        assert_eq!(after_first.run_steps[1].status, RunStepStatus::Running);

        let completed = orchestrator
            .complete_running_step(after_first.run_steps[1].id)
            .expect("complete second");
        assert_eq!(completed.run.status, RunStatus::Completed);
        assert!(completed
            .run_steps
            .iter()
            .all(|step| step.status == RunStepStatus::Completed));
    }

    #[test]
    fn approval_gate_blocks_and_resumes() {
        let (store, orchestrator, project, executor) = setup_store();
        let workflow = store
            .create_workflow(NewWorkflowTemplate {
                project_id: Some(project.id),
                name: "Approval flow".into(),
                description: None,
                steps: vec![NewWorkflowStep {
                    name: "Approval step".into(),
                    instruction: "Wait for approval".into(),
                    order_index: 0,
                    executor_kind: ExecutorKind::NativeCliAi,
                    role_id: None,
                    depends_on_step_id: None,
                    timeout_seconds: None,
                    retry_limit: 0,
                    requires_approval: true,
                    success_criteria: None,
                    artifact_contract: None,
                    input_schema: serde_json::json!({}),
                    output_schema: serde_json::json!({}),
                }],
            })
            .expect("workflow");

        let waiting = orchestrator
            .start_run(NewRun {
                project_id: project.id,
                workflow_template_id: workflow.id,
                executor_profile_id: Some(executor.id),
                goal_id: None,
                compiled_by: None,
                assigned_role_id: None,
                effective_executor_kind: None,
                requested_by: Some("operator".into()),
            })
            .expect("run start");

        assert_eq!(waiting.run.status, RunStatus::WaitingForApproval);
        assert_eq!(
            waiting.run_steps[0].status,
            RunStepStatus::WaitingForApproval
        );
        let approval = waiting.pending_approval.expect("approval gate");

        let approved = orchestrator
            .approve_gate(
                approval.id,
                Some("operator".into()),
                Some("Looks good".into()),
            )
            .expect("approve gate");

        assert_eq!(approved.run.status, RunStatus::Running);
        assert_eq!(approved.run_steps[0].status, RunStepStatus::Running);

        let completed = orchestrator
            .complete_running_step(approved.run_steps[0].id)
            .expect("complete step");
        assert_eq!(completed.run.status, RunStatus::Completed);
    }

    #[test]
    fn recovers_run_state_after_restart() {
        let (store, orchestrator, project, executor) = setup_store();
        let workflow = store
            .create_workflow(NewWorkflowTemplate {
                project_id: Some(project.id),
                name: "Recovery flow".into(),
                description: None,
                steps: vec![NewWorkflowStep {
                    name: "Long step".into(),
                    instruction: "Keep running".into(),
                    order_index: 0,
                    executor_kind: ExecutorKind::Shell,
                    role_id: None,
                    depends_on_step_id: None,
                    timeout_seconds: None,
                    retry_limit: 0,
                    requires_approval: false,
                    success_criteria: None,
                    artifact_contract: None,
                    input_schema: serde_json::json!({}),
                    output_schema: serde_json::json!({}),
                }],
            })
            .expect("workflow");

        let started = orchestrator
            .start_run(NewRun {
                project_id: project.id,
                workflow_template_id: workflow.id,
                executor_profile_id: Some(executor.id),
                goal_id: None,
                compiled_by: None,
                assigned_role_id: None,
                effective_executor_kind: None,
                requested_by: Some("operator".into()),
            })
            .expect("run start");
        assert_eq!(started.run.status, RunStatus::Running);

        let recovered = RunOrchestrator::new(store.clone(), EventBus::default())
            .recover_in_progress_runs()
            .expect("recover");

        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].run.id, started.run.id);
        assert_eq!(recovered[0].run.status, RunStatus::Running);
        assert_eq!(recovered[0].run_steps[0].status, RunStepStatus::Running);
    }

    #[test]
    fn super_owner_compiles_goal_into_workflow() {
        let (store, _orchestrator, project, _executor) = setup_store();
        let ceo = store
            .create_agent_role(domain::NewAgentRole {
                name: "CEO".into(),
                description: Some("Sets direction".into()),
                system_prompt: "Set direction".into(),
                default_executor_kind: Some(ExecutorKind::ClaudeCode),
            })
            .expect("ceo");
        let engineer = store
            .create_agent_role(domain::NewAgentRole {
                name: "Engineer".into(),
                description: Some("Builds things".into()),
                system_prompt: "Build things".into(),
                default_executor_kind: Some(ExecutorKind::Codex),
            })
            .expect("engineer");
        let reviewer = store
            .create_agent_role(domain::NewAgentRole {
                name: "Reviewer".into(),
                description: Some("Reviews changes".into()),
                system_prompt: "Review output".into(),
                default_executor_kind: Some(ExecutorKind::OpenCode),
            })
            .expect("reviewer");
        let pm = store
            .create_agent_role(domain::NewAgentRole {
                name: "PM".into(),
                description: Some("Plans execution".into()),
                system_prompt: "Plan execution".into(),
                default_executor_kind: Some(ExecutorKind::NativeCliAi),
            })
            .expect("pm");
        let skill = store
            .create_skill(domain::NewSkillDefinition {
                name: "Repo awareness".into(),
                description: None,
                instructions: "Read repository guidance before acting.".into(),
                source: domain::SkillSource::Inline,
                source_uri: None,
            })
            .expect("skill");
        store.bind_skill_to_role(pm.id, skill.id).expect("binding");

        let goal = store
            .create_goal(domain::NewGoalSpec {
                project_id: project.id,
                kind: domain::GoalKind::CreateApp,
                title: "Build desktop agent app".into(),
                prompt: "Create a desktop-first orchestration app".into(),
            })
            .expect("goal");

        let compiled = SuperOwner::new(store.clone())
            .compile_goal(goal.id, Some("Use cargo test before shipping."))
            .expect("compile");

        assert_eq!(compiled.goal.status, domain::GoalStatus::Compiled);
        assert_eq!(compiled.workflow.steps.len(), 4);
        assert_eq!(compiled.workflow.steps[0].role_id, Some(ceo.id));
        assert_eq!(compiled.workflow.steps[1].role_id, Some(pm.id));
        assert_eq!(compiled.workflow.steps[2].role_id, Some(engineer.id));
        assert_eq!(compiled.workflow.steps[3].role_id, Some(reviewer.id));
        assert!(compiled.workflow.steps[1]
            .instruction
            .contains("Read repository guidance before acting."));
        assert!(compiled.workflow.steps[1]
            .instruction
            .contains("Use cargo test before shipping."));
    }
}
