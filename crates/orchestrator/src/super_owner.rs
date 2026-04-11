use anyhow::{anyhow, Result};
use domain::{GoalSpec, GoalStatus, Project, SkillDefinition, WorkflowTemplate};
use persistence::OrchestratorStore;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::workflow_compiler::{CompiledRoleInput, CompiledRolePlan, WorkflowCompiler};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompiledGoal {
    pub goal: GoalSpec,
    pub project: Project,
    pub workflow: WorkflowTemplate,
    pub agents_md: Option<String>,
}

#[derive(Clone)]
pub struct SuperOwner {
    store: OrchestratorStore,
    compiler: WorkflowCompiler,
}

impl SuperOwner {
    pub fn new(store: OrchestratorStore) -> Self {
        Self {
            store,
            compiler: WorkflowCompiler,
        }
    }

    pub fn compile_goal(&self, goal_id: Uuid, agents_md: Option<&str>) -> Result<CompiledGoal> {
        let goal = self
            .store
            .get_goal(goal_id)?
            .ok_or_else(|| anyhow!("goal {goal_id} not found"))?;
        let project = self
            .store
            .get_project(goal.project_id)?
            .ok_or_else(|| anyhow!("project {} not found", goal.project_id))?;
        let roles = self.build_role_inputs()?;
        let draft = self
            .compiler
            .compile(&goal, &project, &roles, agents_md);
        let workflow = self.store.create_workflow(draft)?;
        let goal = self
            .store
            .update_goal_compilation(goal.id, workflow.id, GoalStatus::Compiled)?;

        Ok(CompiledGoal {
            goal,
            project,
            workflow,
            agents_md: agents_md.map(str::to_owned),
        })
    }

    fn build_role_inputs(&self) -> Result<Vec<CompiledRoleInput>> {
        let roles = self.store.list_agent_roles()?;
        let skills = self.store.list_skills()?;
        let mut outputs = Vec::with_capacity(roles.len());

        for role in roles {
            let bindings = self.store.list_role_skills(role.id)?;
            let bound_skills = bindings
                .iter()
                .filter_map(|binding| {
                    skills
                        .iter()
                        .find(|skill| skill.id == binding.skill_id)
                        .cloned()
                })
                .collect::<Vec<SkillDefinition>>();
            outputs.push(CompiledRoleInput {
                plan: build_plan_for_role(&role, &bound_skills),
                role,
                skills: bound_skills,
            });
        }

        Ok(outputs)
    }
}

fn build_plan_for_role(
    role: &domain::AgentRole,
    _skills: &[SkillDefinition],
) -> CompiledRolePlan {
    let lower_name = role.name.to_ascii_lowercase();
    if lower_name.contains("ceo") {
        return CompiledRolePlan {
            role_id: Some(role.id),
            role_name: role.name.clone(),
            executor_kind: role
                .default_executor_kind
                .clone()
                .unwrap_or(domain::ExecutorKind::ClaudeCode),
            step_name: "Define product strategy".into(),
            success_criteria: "A clear strategy, constraints, and user-facing success metrics.".into(),
            artifact_contract: "product_strategy".into(),
            instructions: "Clarify the product direction, target outcome, constraints, and the north-star definition of success.".into(),
        };
    }
    if lower_name.contains("pm") {
        return CompiledRolePlan {
            role_id: Some(role.id),
            role_name: role.name.clone(),
            executor_kind: role
                .default_executor_kind
                .clone()
                .unwrap_or(domain::ExecutorKind::NativeCliAi),
            step_name: "Create delivery plan".into(),
            success_criteria: "A delivery plan with milestones, dependencies, and clear artifacts.".into(),
            artifact_contract: "delivery_plan".into(),
            instructions: "Transform the strategy into a practical execution plan with milestones, dependencies, and operator checkpoints.".into(),
        };
    }
    if lower_name.contains("review") {
        return CompiledRolePlan {
            role_id: Some(role.id),
            role_name: role.name.clone(),
            executor_kind: role
                .default_executor_kind
                .clone()
                .unwrap_or(domain::ExecutorKind::OpenCode),
            step_name: "Review output".into(),
            success_criteria: "A concise review with findings, risks, and verification gaps.".into(),
            artifact_contract: "review_summary".into(),
            instructions: "Review the produced output, enumerate findings by severity, and call out remaining risks and missing verification.".into(),
        };
    }

    CompiledRolePlan {
        role_id: Some(role.id),
        role_name: role.name.clone(),
        executor_kind: role
            .default_executor_kind
            .clone()
            .unwrap_or(domain::ExecutorKind::Codex),
        step_name: "Implement solution".into(),
        success_criteria: "A concrete implementation artifact and explanation of what changed.".into(),
        artifact_contract: "implementation_bundle".into(),
        instructions: "Implement the scoped solution, explain assumptions, and produce the main deliverable for the goal.".into(),
    }
}
