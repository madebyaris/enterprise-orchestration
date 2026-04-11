use domain::{
    AgentRole, ExecutorKind, GoalKind, GoalSpec, NewWorkflowStep, NewWorkflowTemplate, Project,
    SkillDefinition,
};

use crate::prompt_builder::{PromptBuilder, RolePromptContext};

#[derive(Debug, Clone)]
pub struct CompiledRolePlan {
    pub role_id: Option<uuid::Uuid>,
    pub role_name: String,
    pub executor_kind: ExecutorKind,
    pub step_name: String,
    pub success_criteria: String,
    pub artifact_contract: String,
    pub instructions: String,
}

#[derive(Clone, Default)]
pub struct WorkflowCompiler;

impl WorkflowCompiler {
    pub fn compile(
        &self,
        goal: &GoalSpec,
        project: &Project,
        roles: &[CompiledRoleInput],
        agents_md: Option<&str>,
    ) -> NewWorkflowTemplate {
        let selected_roles = select_roles(goal.kind.clone(), roles);
        let steps = selected_roles
            .into_iter()
            .enumerate()
            .map(|(index, role)| NewWorkflowStep {
                name: role.plan.step_name.clone(),
                instruction: PromptBuilder::build_step_prompt(
                    &RolePromptContext {
                        name: &role.plan.role_name,
                        description: role.role.description.as_deref(),
                        system_prompt: &role.role.system_prompt,
                        skills: &role.skills,
                    },
                    goal,
                    project,
                    &role.plan.instructions,
                    agents_md,
                ),
                order_index: index as i32,
                executor_kind: role.plan.executor_kind.clone(),
                role_id: role.plan.role_id,
                depends_on_step_id: None,
                timeout_seconds: Some(600),
                retry_limit: 1,
                requires_approval: matches!(role.plan.executor_kind, ExecutorKind::OpenCode),
                success_criteria: Some(role.plan.success_criteria.clone()),
                artifact_contract: Some(role.plan.artifact_contract.clone()),
                input_schema: serde_json::json!({
                    "goal_id": goal.id,
                    "project_id": project.id,
                    "role": role.plan.role_name,
                }),
                output_schema: serde_json::json!({
                    "artifact_contract": role.plan.artifact_contract,
                    "success_criteria": role.plan.success_criteria,
                }),
            })
            .collect();

        NewWorkflowTemplate {
            project_id: Some(project.id),
            name: format!("{} workflow", goal.title),
            description: Some(format!(
                "Compiled by Super Owner for {}",
                goal.kind.as_str()
            )),
            steps,
        }
    }
}

#[derive(Clone)]
pub struct CompiledRoleInput {
    pub role: AgentRole,
    pub skills: Vec<SkillDefinition>,
    pub plan: CompiledRolePlan,
}

fn select_roles(goal_kind: GoalKind, roles: &[CompiledRoleInput]) -> Vec<CompiledRoleInput> {
    let desired = match goal_kind {
        GoalKind::CreateApp => ["ceo", "pm", "engineer", "reviewer"].as_slice(),
        GoalKind::CreateWorkflow => ["pm", "engineer", "reviewer"].as_slice(),
    };

    let mut selected = Vec::new();
    for wanted in desired {
        if let Some(role) = roles
            .iter()
            .find(|role| role.role.name.to_ascii_lowercase().contains(wanted))
        {
            selected.push(role.clone());
        } else if let Some(fallback) = fallback_role(wanted) {
            selected.push(fallback);
        }
    }
    selected
}

fn fallback_role(wanted: &str) -> Option<CompiledRoleInput> {
    let (name, prompt, executor_kind, step_name, success_criteria, artifact_contract, instructions) =
        match wanted {
            "ceo" => (
                "CEO",
                "Set strategy, constraints, and the target outcome for the product initiative.",
                ExecutorKind::ClaudeCode,
                "Define product strategy",
                "A concise product strategy with success metrics and risk framing.",
                "product_strategy",
                "Clarify the intended app outcome, user value, constraints, and acceptance criteria.",
            ),
            "pm" => (
                "PM",
                "Break the initiative into milestones, execution phases, and explicit deliverables.",
                ExecutorKind::NativeCliAi,
                "Create delivery plan",
                "A stepwise implementation plan with milestones and handoffs.",
                "delivery_plan",
                "Turn the goal into an operator-ready implementation plan with milestones and dependencies.",
            ),
            "engineer" => (
                "Engineer",
                "Implement or scaffold the solution using the selected tools and constraints.",
                ExecutorKind::Codex,
                "Implement solution",
                "A concrete implementation or scaffold with validation notes.",
                "implementation_bundle",
                "Implement the scoped solution, calling out assumptions and artifacts that were produced.",
            ),
            "reviewer" => (
                "Reviewer",
                "Review implementation quality, risks, missing tests, and ship readiness.",
                ExecutorKind::OpenCode,
                "Review output",
                "A review summary with findings, residual risks, and explicit verification notes.",
                "review_summary",
                "Review the produced artifacts, verify quality, and report risks before release.",
            ),
            _ => return None,
        };

    Some(CompiledRoleInput {
        role: AgentRole {
            id: uuid::Uuid::nil(),
            name: name.into(),
            description: None,
            system_prompt: prompt.into(),
            default_executor_kind: Some(executor_kind.clone()),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        },
        skills: Vec::new(),
        plan: CompiledRolePlan {
            role_id: None,
            role_name: name.into(),
            executor_kind,
            step_name: step_name.into(),
            success_criteria: success_criteria.into(),
            artifact_contract: artifact_contract.into(),
            instructions: instructions.into(),
        },
    })
}
