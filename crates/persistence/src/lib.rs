use std::{
    path::Path,
    str::FromStr,
    sync::{Arc, Mutex, MutexGuard},
};

use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use domain::{
    AgentRole, ApprovalDecision, ApprovalGate, Artifact, EventEnvelope, EventScope, ExecutorKind,
    ExecutorProfile, GoalKind, GoalSpec, GoalStatus, NewAgentRole, NewExecutorProfile, NewGoalSpec,
    NewOrganizationTemplate, NewProject, NewRun, NewSkillDefinition, NewWorkflowTemplate,
    OrganizationTemplate, PairingSession, Project, Run, RunStatus, RunStep, RunStepStatus,
    SkillBinding, SkillDefinition, SkillSource, WorkflowStep, WorkflowTemplate,
};
use rusqlite::{params, Connection, OptionalExtension, Row};
use uuid::Uuid;

const INITIAL_MIGRATION: &str = include_str!("../migrations/0001_initial.sql");
const SUPER_OWNER_MIGRATION: &str = include_str!("../migrations/0002_super_owner.sql");

#[derive(Clone)]
pub struct OrchestratorStore {
    connection: Arc<Mutex<Connection>>,
}

impl OrchestratorStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!(
                    "failed to create database parent directory {}",
                    parent.display()
                )
            })?;
        }

        let connection = Connection::open(path)
            .with_context(|| format!("failed to open database at {}", path.display()))?;
        let store = Self {
            connection: Arc::new(Mutex::new(connection)),
        };
        store.run_migrations()?;
        Ok(store)
    }

    pub fn open_in_memory() -> Result<Self> {
        let store = Self {
            connection: Arc::new(Mutex::new(Connection::open_in_memory()?)),
        };
        store.run_migrations()?;
        Ok(store)
    }

    pub fn run_migrations(&self) -> Result<()> {
        let connection = self.connection()?;
        connection.execute_batch(INITIAL_MIGRATION)?;
        for statement in SUPER_OWNER_MIGRATION
            .split(';')
            .map(str::trim)
            .filter(|statement| !statement.is_empty())
        {
            if let Err(error) = connection.execute(statement, []) {
                let message = error.to_string();
                let ignorable = message.contains("duplicate column name")
                    || message.contains("already exists");
                if !ignorable {
                    return Err(error.into());
                }
            }
        }
        Ok(())
    }

    pub fn create_project(&self, input: NewProject) -> Result<Project> {
        let project = Project {
            id: Uuid::new_v4(),
            name: input.name,
            description: input.description,
            workspace_path: input.workspace_path,
            repository_url: input.repository_url,
            default_executor_profile_id: input.default_executor_profile_id,
            agents_md_path: None,
            agents_md_updated_at: None,
            archived_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO projects
            (id, name, description, workspace_path, repository_url, default_executor_profile_id, agents_md_path, agents_md_updated_at, archived_at, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                project.id.to_string(),
                &project.name,
                project.description.as_deref(),
                &project.workspace_path,
                project.repository_url.as_deref(),
                project.default_executor_profile_id.map(|value| value.to_string()),
                project.agents_md_path.as_deref(),
                project.agents_md_updated_at.map(to_timestamp),
                project.archived_at.map(to_timestamp),
                to_timestamp(project.created_at),
                to_timestamp(project.updated_at),
            ],
        )?;

        Ok(project)
    }

    pub fn list_projects(&self) -> Result<Vec<Project>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, name, description, workspace_path, repository_url, default_executor_profile_id, agents_md_path, agents_md_updated_at, archived_at, created_at, updated_at
             FROM projects
             ORDER BY created_at DESC",
        )?;

        let rows = statement.query_map([], |row| {
            Ok(Project {
                id: parse_uuid(row.get::<_, String>(0)?)?,
                name: row.get(1)?,
                description: row.get(2)?,
                workspace_path: row.get(3)?,
                repository_url: row.get(4)?,
                default_executor_profile_id: parse_optional_uuid(row.get::<_, Option<String>>(5)?)?,
                agents_md_path: row.get(6)?,
                agents_md_updated_at: parse_optional_datetime(row.get::<_, Option<String>>(7)?)?,
                archived_at: parse_optional_datetime(row.get::<_, Option<String>>(8)?)?,
                created_at: parse_datetime(&row.get::<_, String>(9)?)?,
                updated_at: parse_datetime(&row.get::<_, String>(10)?)?,
            })
        })?;

        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn get_project(&self, project_id: Uuid) -> Result<Option<Project>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, name, description, workspace_path, repository_url, default_executor_profile_id, agents_md_path, agents_md_updated_at, archived_at, created_at, updated_at
             FROM projects
             WHERE id = ?1",
        )?;

        statement
            .query_row([project_id.to_string()], |row| {
                Ok(Project {
                    id: parse_uuid(row.get::<_, String>(0)?)?,
                    name: row.get(1)?,
                    description: row.get(2)?,
                    workspace_path: row.get(3)?,
                    repository_url: row.get(4)?,
                    default_executor_profile_id: parse_optional_uuid(row.get::<_, Option<String>>(5)?)?,
                    agents_md_path: row.get(6)?,
                    agents_md_updated_at: parse_optional_datetime(row.get::<_, Option<String>>(7)?)?,
                    archived_at: parse_optional_datetime(row.get::<_, Option<String>>(8)?)?,
                    created_at: parse_datetime(&row.get::<_, String>(9)?)?,
                    updated_at: parse_datetime(&row.get::<_, String>(10)?)?,
                })
            })
            .optional()
            .map_err(Into::into)
    }

    pub fn get_executor_profile(&self, profile_id: Uuid) -> Result<Option<ExecutorProfile>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, name, kind, binary_path, config_json, created_at, updated_at
             FROM executor_profiles
             WHERE id = ?1",
        )?;

        statement
            .query_row([profile_id.to_string()], |row| {
                let config_json: String = row.get(4)?;
                Ok(ExecutorProfile {
                    id: parse_uuid(row.get::<_, String>(0)?)?,
                    name: row.get(1)?,
                    kind: ExecutorKind::from_str(&row.get::<_, String>(2)?)
                        .map_err(to_sql_conversion_error)?,
                    binary_path: row.get(3)?,
                    config_json: serde_json::from_str(&config_json).map_err(to_sql_conversion_error)?,
                    created_at: parse_datetime(&row.get::<_, String>(5)?)?,
                    updated_at: parse_datetime(&row.get::<_, String>(6)?)?,
                })
            })
            .optional()
            .map_err(Into::into)
    }

    pub fn create_executor_profile(&self, input: NewExecutorProfile) -> Result<ExecutorProfile> {
        let profile = ExecutorProfile {
            id: Uuid::new_v4(),
            name: input.name,
            kind: input.kind,
            binary_path: input.binary_path,
            config_json: input.config_json,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO executor_profiles
            (id, name, kind, binary_path, config_json, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                profile.id.to_string(),
                &profile.name,
                profile.kind.as_str(),
                profile.binary_path.as_deref(),
                serde_json::to_string(&profile.config_json)?,
                to_timestamp(profile.created_at),
                to_timestamp(profile.updated_at),
            ],
        )?;

        Ok(profile)
    }

    pub fn list_executor_profiles(&self) -> Result<Vec<ExecutorProfile>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, name, kind, binary_path, config_json, created_at, updated_at
             FROM executor_profiles
             ORDER BY created_at DESC",
        )?;

        let rows = statement.query_map([], |row| {
            let config_json: String = row.get(4)?;
            Ok(ExecutorProfile {
                id: parse_uuid(row.get::<_, String>(0)?)?,
                name: row.get(1)?,
                kind: ExecutorKind::from_str(&row.get::<_, String>(2)?)
                    .map_err(to_sql_conversion_error)?,
                binary_path: row.get(3)?,
                config_json: serde_json::from_str(&config_json).map_err(to_sql_conversion_error)?,
                created_at: parse_datetime(&row.get::<_, String>(5)?)?,
                updated_at: parse_datetime(&row.get::<_, String>(6)?)?,
            })
        })?;

        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn update_project_agents_md(
        &self,
        project_id: Uuid,
        agents_md_path: Option<String>,
        agents_md_updated_at: Option<DateTime<Utc>>,
    ) -> Result<Project> {
        let updated_at = Utc::now();
        {
            let connection = self.connection()?;
            connection.execute(
                "UPDATE projects
                 SET agents_md_path = ?2, agents_md_updated_at = ?3, updated_at = ?4
                 WHERE id = ?1",
                params![
                    project_id.to_string(),
                    agents_md_path.as_deref(),
                    agents_md_updated_at.map(to_timestamp),
                    to_timestamp(updated_at),
                ],
            )?;
        }

        self.get_project(project_id)?
            .ok_or_else(|| anyhow!("project {project_id} not found after update"))
    }

    pub fn create_agent_role(&self, input: NewAgentRole) -> Result<AgentRole> {
        let role = AgentRole {
            id: Uuid::new_v4(),
            name: input.name,
            description: input.description,
            system_prompt: input.system_prompt,
            default_executor_kind: input.default_executor_kind,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO agent_roles
             (id, name, description, system_prompt, default_executor_kind, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                role.id.to_string(),
                &role.name,
                role.description.as_deref(),
                &role.system_prompt,
                role.default_executor_kind.as_ref().map(ExecutorKind::as_str),
                to_timestamp(role.created_at),
                to_timestamp(role.updated_at),
            ],
        )?;

        Ok(role)
    }

    pub fn list_agent_roles(&self) -> Result<Vec<AgentRole>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, name, description, system_prompt, default_executor_kind, created_at, updated_at
             FROM agent_roles
             ORDER BY created_at DESC",
        )?;

        let rows = statement.query_map([], map_agent_role)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn get_agent_role(&self, role_id: Uuid) -> Result<Option<AgentRole>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, name, description, system_prompt, default_executor_kind, created_at, updated_at
             FROM agent_roles
             WHERE id = ?1",
        )?;

        statement
            .query_row([role_id.to_string()], map_agent_role)
            .optional()
            .map_err(Into::into)
    }

    pub fn create_skill(&self, input: NewSkillDefinition) -> Result<SkillDefinition> {
        let skill = SkillDefinition {
            id: Uuid::new_v4(),
            name: input.name,
            description: input.description,
            instructions: input.instructions,
            source: input.source,
            source_uri: input.source_uri,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO skills
             (id, name, description, instructions, source, source_uri, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                skill.id.to_string(),
                &skill.name,
                skill.description.as_deref(),
                &skill.instructions,
                skill.source.as_str(),
                skill.source_uri.as_deref(),
                to_timestamp(skill.created_at),
                to_timestamp(skill.updated_at),
            ],
        )?;

        Ok(skill)
    }

    pub fn list_skills(&self) -> Result<Vec<SkillDefinition>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, name, description, instructions, source, source_uri, created_at, updated_at
             FROM skills
             ORDER BY created_at DESC",
        )?;

        let rows = statement.query_map([], map_skill)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn bind_skill_to_role(&self, role_id: Uuid, skill_id: Uuid) -> Result<SkillBinding> {
        let binding = SkillBinding {
            role_id,
            skill_id,
            created_at: Utc::now(),
        };

        let connection = self.connection()?;
        connection.execute(
            "INSERT OR IGNORE INTO role_skills (role_id, skill_id, created_at)
             VALUES (?1, ?2, ?3)",
            params![
                binding.role_id.to_string(),
                binding.skill_id.to_string(),
                to_timestamp(binding.created_at),
            ],
        )?;

        Ok(binding)
    }

    pub fn list_role_skills(&self, role_id: Uuid) -> Result<Vec<SkillBinding>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT role_id, skill_id, created_at
             FROM role_skills
             WHERE role_id = ?1
             ORDER BY created_at ASC",
        )?;

        let rows = statement.query_map([role_id.to_string()], |row| {
            Ok(SkillBinding {
                role_id: parse_uuid(row.get::<_, String>(0)?)?,
                skill_id: parse_uuid(row.get::<_, String>(1)?)?,
                created_at: parse_datetime(&row.get::<_, String>(2)?)?,
            })
        })?;

        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn create_goal(&self, input: NewGoalSpec) -> Result<GoalSpec> {
        let goal = GoalSpec {
            id: Uuid::new_v4(),
            project_id: input.project_id,
            kind: input.kind,
            title: input.title,
            prompt: input.prompt,
            status: GoalStatus::Draft,
            compiled_workflow_template_id: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO goals
             (id, project_id, kind, title, prompt, status, compiled_workflow_template_id, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                goal.id.to_string(),
                goal.project_id.to_string(),
                goal.kind.as_str(),
                &goal.title,
                &goal.prompt,
                goal.status.as_str(),
                goal.compiled_workflow_template_id.map(|value| value.to_string()),
                to_timestamp(goal.created_at),
                to_timestamp(goal.updated_at),
            ],
        )?;

        Ok(goal)
    }

    pub fn list_goals(&self) -> Result<Vec<GoalSpec>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, project_id, kind, title, prompt, status, compiled_workflow_template_id, created_at, updated_at
             FROM goals
             ORDER BY created_at DESC",
        )?;

        let rows = statement.query_map([], map_goal)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn get_goal(&self, goal_id: Uuid) -> Result<Option<GoalSpec>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, project_id, kind, title, prompt, status, compiled_workflow_template_id, created_at, updated_at
             FROM goals
             WHERE id = ?1",
        )?;

        statement
            .query_row([goal_id.to_string()], map_goal)
            .optional()
            .map_err(Into::into)
    }

    pub fn update_goal_compilation(
        &self,
        goal_id: Uuid,
        workflow_template_id: Uuid,
        status: GoalStatus,
    ) -> Result<GoalSpec> {
        let updated_at = Utc::now();
        {
            let connection = self.connection()?;
            connection.execute(
                "UPDATE goals
                 SET compiled_workflow_template_id = ?2, status = ?3, updated_at = ?4
                 WHERE id = ?1",
                params![
                    goal_id.to_string(),
                    workflow_template_id.to_string(),
                    status.as_str(),
                    to_timestamp(updated_at),
                ],
            )?;
        }

        self.get_goal(goal_id)?
            .ok_or_else(|| anyhow!("goal {goal_id} not found after update"))
    }

    pub fn create_organization_template(
        &self,
        input: NewOrganizationTemplate,
    ) -> Result<OrganizationTemplate> {
        let organization = OrganizationTemplate {
            id: Uuid::new_v4(),
            name: input.name,
            description: input.description,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO organization_templates (id, name, description, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                organization.id.to_string(),
                &organization.name,
                organization.description.as_deref(),
                to_timestamp(organization.created_at),
                to_timestamp(organization.updated_at),
            ],
        )?;

        Ok(organization)
    }

    pub fn list_organization_templates(&self) -> Result<Vec<OrganizationTemplate>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, name, description, created_at, updated_at
             FROM organization_templates
             ORDER BY created_at DESC",
        )?;

        let rows = statement.query_map([], |row| {
            Ok(OrganizationTemplate {
                id: parse_uuid(row.get::<_, String>(0)?)?,
                name: row.get(1)?,
                description: row.get(2)?,
                created_at: parse_datetime(&row.get::<_, String>(3)?)?,
                updated_at: parse_datetime(&row.get::<_, String>(4)?)?,
            })
        })?;

        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn create_workflow(&self, input: NewWorkflowTemplate) -> Result<WorkflowTemplate> {
        let workflow = WorkflowTemplate {
            id: Uuid::new_v4(),
            project_id: input.project_id,
            name: input.name,
            description: input.description,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            steps: input
                .steps
                .into_iter()
                .map(|step| WorkflowStep {
                    id: Uuid::new_v4(),
                    workflow_template_id: Uuid::nil(),
                    name: step.name,
                    instruction: step.instruction,
                    order_index: step.order_index,
                    executor_kind: step.executor_kind,
                    role_id: step.role_id,
                    depends_on_step_id: step.depends_on_step_id,
                    timeout_seconds: step.timeout_seconds,
                    retry_limit: step.retry_limit,
                    requires_approval: step.requires_approval,
                    success_criteria: step.success_criteria,
                    artifact_contract: step.artifact_contract,
                    input_schema: step.input_schema,
                    output_schema: step.output_schema,
                })
                .collect(),
        };

        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO workflow_templates
            (id, project_id, name, description, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                workflow.id.to_string(),
                workflow.project_id.map(|value| value.to_string()),
                &workflow.name,
                workflow.description.as_deref(),
                to_timestamp(workflow.created_at),
                to_timestamp(workflow.updated_at),
            ],
        )?;

        for step in &workflow.steps {
            transaction.execute(
                "INSERT INTO workflow_steps
                (id, workflow_template_id, name, instruction, order_index, executor_kind, role_id, depends_on_step_id, timeout_seconds, retry_limit, requires_approval, success_criteria, artifact_contract, input_schema_json, output_schema_json)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                params![
                    step.id.to_string(),
                    workflow.id.to_string(),
                    &step.name,
                    &step.instruction,
                    step.order_index,
                    step.executor_kind.as_str(),
                    step.role_id.map(|value| value.to_string()),
                    step.depends_on_step_id.map(|value| value.to_string()),
                    step.timeout_seconds,
                    step.retry_limit,
                    step.requires_approval as i64,
                    step.success_criteria.as_deref(),
                    step.artifact_contract.as_deref(),
                    serde_json::to_string(&step.input_schema)?,
                    serde_json::to_string(&step.output_schema)?,
                ],
            )?;
        }

        transaction.commit()?;

        Ok(WorkflowTemplate {
            steps: workflow
                .steps
                .into_iter()
                .map(|step| WorkflowStep {
                    workflow_template_id: workflow.id,
                    ..step
                })
                .collect(),
            ..workflow
        })
    }

    pub fn list_workflows(&self) -> Result<Vec<WorkflowTemplate>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, project_id, name, description, created_at, updated_at
             FROM workflow_templates
             ORDER BY created_at DESC",
        )?;

        let template_rows = statement.query_map([], |row| {
            Ok(WorkflowTemplate {
                id: parse_uuid(row.get::<_, String>(0)?)?,
                project_id: parse_optional_uuid(row.get::<_, Option<String>>(1)?)?,
                name: row.get(2)?,
                description: row.get(3)?,
                created_at: parse_datetime(&row.get::<_, String>(4)?)?,
                updated_at: parse_datetime(&row.get::<_, String>(5)?)?,
                steps: Vec::new(),
            })
        })?;

        let mut templates = template_rows.collect::<rusqlite::Result<Vec<_>>>()?;
        drop(statement);
        drop(connection);
        for template in &mut templates {
            template.steps = self.list_workflow_steps(template.id)?;
        }

        Ok(templates)
    }

    pub fn create_run(&self, input: NewRun) -> Result<Run> {
        let run = Run {
            id: Uuid::new_v4(),
            project_id: input.project_id,
            workflow_template_id: input.workflow_template_id,
            executor_profile_id: input.executor_profile_id,
            goal_id: input.goal_id,
            compiled_by: input.compiled_by,
            assigned_role_id: input.assigned_role_id,
            effective_executor_kind: input.effective_executor_kind,
            status: RunStatus::Queued,
            requested_by: input.requested_by,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let workflow_steps = self.list_workflow_steps(run.workflow_template_id)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO runs
            (id, project_id, workflow_template_id, executor_profile_id, goal_id, compiled_by, assigned_role_id, effective_executor_kind, status, requested_by, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                run.id.to_string(),
                run.project_id.to_string(),
                run.workflow_template_id.to_string(),
                run.executor_profile_id.map(|value| value.to_string()),
                run.goal_id.map(|value| value.to_string()),
                run.compiled_by.as_deref(),
                run.assigned_role_id.map(|value| value.to_string()),
                run.effective_executor_kind.as_ref().map(ExecutorKind::as_str),
                run.status.as_str(),
                run.requested_by.as_deref(),
                to_timestamp(run.created_at),
                to_timestamp(run.updated_at),
            ],
        )?;

        for step in workflow_steps {
            let run_step = RunStep {
                id: Uuid::new_v4(),
                run_id: run.id,
                workflow_step_id: step.id,
                status: RunStepStatus::Pending,
                external_session_id: None,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            };

            transaction.execute(
                "INSERT INTO run_steps
                (id, run_id, workflow_step_id, status, external_session_id, created_at, updated_at)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    run_step.id.to_string(),
                    run_step.run_id.to_string(),
                    run_step.workflow_step_id.to_string(),
                    run_step.status.as_str(),
                    run_step.external_session_id,
                    to_timestamp(run_step.created_at),
                    to_timestamp(run_step.updated_at),
                ],
            )?;
        }

        transaction.commit()?;

        Ok(run)
    }

    pub fn list_runs(&self) -> Result<Vec<Run>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, project_id, workflow_template_id, executor_profile_id, goal_id, compiled_by, assigned_role_id, effective_executor_kind, status, requested_by, created_at, updated_at
             FROM runs
             ORDER BY created_at DESC",
        )?;

        let rows = statement.query_map([], |row| {
            Ok(Run {
                id: parse_uuid(row.get::<_, String>(0)?)?,
                project_id: parse_uuid(row.get::<_, String>(1)?)?,
                workflow_template_id: parse_uuid(row.get::<_, String>(2)?)?,
                executor_profile_id: parse_optional_uuid(row.get::<_, Option<String>>(3)?)?,
                goal_id: parse_optional_uuid(row.get::<_, Option<String>>(4)?)?,
                compiled_by: row.get(5)?,
                assigned_role_id: parse_optional_uuid(row.get::<_, Option<String>>(6)?)?,
                effective_executor_kind: row
                    .get::<_, Option<String>>(7)?
                    .map(|value| ExecutorKind::from_str(&value).map_err(to_sql_conversion_error))
                    .transpose()?,
                status: RunStatus::from_str(&row.get::<_, String>(8)?)
                    .map_err(to_sql_conversion_error)?,
                requested_by: row.get(9)?,
                created_at: parse_datetime(&row.get::<_, String>(10)?)?,
                updated_at: parse_datetime(&row.get::<_, String>(11)?)?,
            })
        })?;

        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn get_run(&self, run_id: Uuid) -> Result<Option<Run>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, project_id, workflow_template_id, executor_profile_id, goal_id, compiled_by, assigned_role_id, effective_executor_kind, status, requested_by, created_at, updated_at
             FROM runs
             WHERE id = ?1",
        )?;

        statement
            .query_row([run_id.to_string()], map_run)
            .optional()
            .map_err(Into::into)
    }

    pub fn update_run_status(&self, run_id: Uuid, status: RunStatus) -> Result<Run> {
        let updated_at = Utc::now();
        {
            let connection = self.connection()?;
            connection.execute(
                "UPDATE runs SET status = ?2, updated_at = ?3 WHERE id = ?1",
                params![
                    run_id.to_string(),
                    status.as_str(),
                    to_timestamp(updated_at)
                ],
            )?;
        }

        self.get_run(run_id)?
            .ok_or_else(|| anyhow!("run {run_id} not found after update"))
    }

    pub fn list_run_steps(&self, run_id: Uuid) -> Result<Vec<RunStep>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, run_id, workflow_step_id, status, external_session_id, created_at, updated_at
             FROM run_steps
             WHERE run_id = ?1
             ORDER BY created_at ASC",
        )?;

        let rows = statement.query_map([run_id.to_string()], map_run_step)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn get_run_step(&self, run_step_id: Uuid) -> Result<Option<RunStep>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, run_id, workflow_step_id, status, external_session_id, created_at, updated_at
             FROM run_steps
             WHERE id = ?1",
        )?;

        statement
            .query_row([run_step_id.to_string()], map_run_step)
            .optional()
            .map_err(Into::into)
    }

    pub fn update_run_step_status(
        &self,
        run_step_id: Uuid,
        status: RunStepStatus,
        external_session_id: Option<String>,
    ) -> Result<RunStep> {
        let updated_at = Utc::now();
        {
            let connection = self.connection()?;
            connection.execute(
                "UPDATE run_steps SET status = ?2, external_session_id = ?3, updated_at = ?4 WHERE id = ?1",
                params![
                    run_step_id.to_string(),
                    status.as_str(),
                    external_session_id.as_deref(),
                    to_timestamp(updated_at),
                ],
            )?;
        }

        self.get_run_step(run_step_id)?
            .ok_or_else(|| anyhow!("run step {run_step_id} not found after update"))
    }

    pub fn get_workflow_step(&self, workflow_step_id: Uuid) -> Result<Option<WorkflowStep>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, workflow_template_id, name, instruction, order_index, executor_kind, role_id, depends_on_step_id, timeout_seconds, retry_limit, requires_approval, success_criteria, artifact_contract, input_schema_json, output_schema_json
             FROM workflow_steps
             WHERE id = ?1",
        )?;

        statement
            .query_row([workflow_step_id.to_string()], map_workflow_step)
            .optional()
            .map_err(Into::into)
    }

    pub fn create_approval_gate(
        &self,
        run_id: Uuid,
        run_step_id: Option<Uuid>,
        requested_by: Option<String>,
    ) -> Result<ApprovalGate> {
        let gate = ApprovalGate {
            id: Uuid::new_v4(),
            run_id,
            run_step_id,
            status: ApprovalDecision::Pending,
            requested_by,
            resolved_by: None,
            notes: None,
            created_at: Utc::now(),
            resolved_at: None,
        };

        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO approvals
            (id, run_id, run_step_id, status, requested_by, resolved_by, notes, created_at, resolved_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                gate.id.to_string(),
                gate.run_id.to_string(),
                gate.run_step_id.map(|value| value.to_string()),
                gate.status.as_str(),
                gate.requested_by.as_deref(),
                gate.resolved_by.as_deref(),
                gate.notes.as_deref(),
                to_timestamp(gate.created_at),
                gate.resolved_at.map(to_timestamp),
            ],
        )?;

        Ok(gate)
    }

    pub fn list_approval_gates(&self) -> Result<Vec<ApprovalGate>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, run_id, run_step_id, status, requested_by, resolved_by, notes, created_at, resolved_at
             FROM approvals
             ORDER BY created_at DESC",
        )?;

        let rows = statement.query_map([], |row| {
            Ok(ApprovalGate {
                id: parse_uuid(row.get::<_, String>(0)?)?,
                run_id: parse_uuid(row.get::<_, String>(1)?)?,
                run_step_id: parse_optional_uuid(row.get::<_, Option<String>>(2)?)?,
                status: ApprovalDecision::from_str(&row.get::<_, String>(3)?)
                    .map_err(to_sql_conversion_error)?,
                requested_by: row.get(4)?,
                resolved_by: row.get(5)?,
                notes: row.get(6)?,
                created_at: parse_datetime(&row.get::<_, String>(7)?)?,
                resolved_at: parse_optional_datetime(row.get::<_, Option<String>>(8)?)?,
            })
        })?;

        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn get_approval_gate(&self, approval_id: Uuid) -> Result<Option<ApprovalGate>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, run_id, run_step_id, status, requested_by, resolved_by, notes, created_at, resolved_at
             FROM approvals
             WHERE id = ?1",
        )?;

        statement
            .query_row([approval_id.to_string()], map_approval_gate)
            .optional()
            .map_err(Into::into)
    }

    pub fn find_pending_approval_for_run_step(
        &self,
        run_step_id: Uuid,
    ) -> Result<Option<ApprovalGate>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, run_id, run_step_id, status, requested_by, resolved_by, notes, created_at, resolved_at
             FROM approvals
             WHERE run_step_id = ?1 AND status = 'pending'
             ORDER BY created_at DESC
             LIMIT 1",
        )?;

        statement
            .query_row([run_step_id.to_string()], map_approval_gate)
            .optional()
            .map_err(Into::into)
    }

    pub fn update_approval_gate(
        &self,
        approval_id: Uuid,
        status: ApprovalDecision,
        resolved_by: Option<String>,
        notes: Option<String>,
    ) -> Result<ApprovalGate> {
        let resolved_at = match status {
            ApprovalDecision::Pending => None,
            _ => Some(Utc::now()),
        };
        {
            let connection = self.connection()?;
            connection.execute(
                "UPDATE approvals
                 SET status = ?2, resolved_by = ?3, notes = ?4, resolved_at = ?5
                 WHERE id = ?1",
                params![
                    approval_id.to_string(),
                    status.as_str(),
                    resolved_by.as_deref(),
                    notes.as_deref(),
                    resolved_at.map(to_timestamp),
                ],
            )?;
        }

        self.get_approval_gate(approval_id)?
            .ok_or_else(|| anyhow!("approval gate {approval_id} not found after update"))
    }

    pub fn create_artifact(
        &self,
        run_id: Uuid,
        run_step_id: Option<Uuid>,
        name: impl Into<String>,
        kind: impl Into<String>,
        content_type: Option<String>,
        metadata_json: serde_json::Value,
    ) -> Result<Artifact> {
        let artifact = Artifact {
            id: Uuid::new_v4(),
            run_id,
            run_step_id,
            name: name.into(),
            kind: kind.into(),
            path: None,
            content_type,
            metadata_json,
            created_at: Utc::now(),
        };

        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO artifacts
            (id, run_id, run_step_id, name, kind, path, content_type, metadata_json, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                artifact.id.to_string(),
                artifact.run_id.to_string(),
                artifact.run_step_id.map(|value| value.to_string()),
                &artifact.name,
                &artifact.kind,
                artifact.path.as_deref(),
                artifact.content_type.as_deref(),
                serde_json::to_string(&artifact.metadata_json)?,
                to_timestamp(artifact.created_at),
            ],
        )?;

        Ok(artifact)
    }

    pub fn list_artifacts_for_run(&self, run_id: Uuid) -> Result<Vec<Artifact>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, run_id, run_step_id, name, kind, path, content_type, metadata_json, created_at
             FROM artifacts
             WHERE run_id = ?1
             ORDER BY created_at DESC",
        )?;

        let rows = statement.query_map([run_id.to_string()], map_artifact)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn create_pairing_session(
        &self,
        label: Option<String>,
        expires_at: Option<DateTime<Utc>>,
    ) -> Result<PairingSession> {
        let session = PairingSession {
            id: Uuid::new_v4(),
            token: Uuid::new_v4().simple().to_string(),
            label,
            is_revoked: false,
            created_at: Utc::now(),
            expires_at,
        };

        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO pairing_sessions
            (id, token, label, is_revoked, created_at, expires_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                session.id.to_string(),
                session.token,
                session.label.as_deref(),
                session.is_revoked as i64,
                to_timestamp(session.created_at),
                session.expires_at.map(to_timestamp),
            ],
        )?;

        Ok(session)
    }

    pub fn list_pairing_sessions(&self) -> Result<Vec<PairingSession>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, token, label, is_revoked, created_at, expires_at
             FROM pairing_sessions
             ORDER BY created_at DESC",
        )?;

        let rows = statement.query_map([], map_pairing_session)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn find_active_pairing_session_by_token(
        &self,
        token: &str,
    ) -> Result<Option<PairingSession>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, token, label, is_revoked, created_at, expires_at
             FROM pairing_sessions
             WHERE token = ?1 AND is_revoked = 0
             LIMIT 1",
        )?;

        let session = statement
            .query_row([token], map_pairing_session)
            .optional()?;

        Ok(session.and_then(|value| {
            if value
                .expires_at
                .map(|expires_at| expires_at < Utc::now())
                .unwrap_or(false)
            {
                None
            } else {
                Some(value)
            }
        }))
    }

    pub fn revoke_pairing_session(&self, pairing_id: Uuid) -> Result<PairingSession> {
        {
            let connection = self.connection()?;
            connection.execute(
                "UPDATE pairing_sessions SET is_revoked = 1 WHERE id = ?1",
                params![pairing_id.to_string()],
            )?;
        }

        self.list_pairing_sessions()?
            .into_iter()
            .find(|session| session.id == pairing_id)
            .ok_or_else(|| anyhow!("pairing session {pairing_id} not found after revoke"))
    }

    pub fn record_event(&self, event: &EventEnvelope) -> Result<()> {
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO events (id, scope, event_type, summary, payload_json, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                event.id.to_string(),
                event.scope.as_str(),
                &event.event_type,
                &event.summary,
                serde_json::to_string(&event.payload)?,
                to_timestamp(event.created_at),
            ],
        )?;

        Ok(())
    }

    pub fn list_events(&self) -> Result<Vec<EventEnvelope>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, scope, event_type, summary, payload_json, created_at
             FROM events
             ORDER BY created_at DESC",
        )?;

        let rows = statement.query_map([], |row| {
            let payload_json: String = row.get(4)?;
            Ok(EventEnvelope {
                id: parse_uuid(row.get::<_, String>(0)?)?,
                scope: EventScope::from_str(&row.get::<_, String>(1)?)
                    .map_err(to_sql_conversion_error)?,
                event_type: row.get(2)?,
                summary: row.get(3)?,
                payload: serde_json::from_str(&payload_json).map_err(to_sql_conversion_error)?,
                created_at: parse_datetime(&row.get::<_, String>(5)?)?,
            })
        })?;

        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn list_workflow_steps(&self, workflow_template_id: Uuid) -> Result<Vec<WorkflowStep>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, workflow_template_id, name, instruction, order_index, executor_kind, role_id, depends_on_step_id, timeout_seconds, retry_limit, requires_approval, success_criteria, artifact_contract, input_schema_json, output_schema_json
             FROM workflow_steps
             WHERE workflow_template_id = ?1
             ORDER BY order_index ASC",
        )?;

        let rows = statement.query_map([workflow_template_id.to_string()], map_workflow_step)?;

        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn update_run_step_external_session(
        &self,
        step_id: Uuid,
        session_id: Option<String>,
    ) -> Result<()> {
        let connection = self.connection()?;
        connection.execute(
            "UPDATE run_steps SET external_session_id = ?1, updated_at = CURRENT_TIMESTAMP WHERE id = ?2",
            params![session_id, step_id.to_string()],
        )?;
        Ok(())
    }

    pub fn execute(&self, sql: &str, params: impl rusqlite::Params) -> Result<()> {
        let connection = self.connection()?;
        connection.execute(sql, params)?;
        Ok(())
    }

    fn connection(&self) -> Result<MutexGuard<'_, Connection>> {
        self.connection
            .lock()
            .map_err(|_| anyhow!("database mutex is poisoned"))
    }
}

fn parse_uuid(value: String) -> rusqlite::Result<Uuid> {
    Uuid::parse_str(&value).map_err(to_sql_conversion_error)
}

fn map_run(row: &Row<'_>) -> rusqlite::Result<Run> {
    Ok(Run {
        id: parse_uuid(row.get::<_, String>(0)?)?,
        project_id: parse_uuid(row.get::<_, String>(1)?)?,
        workflow_template_id: parse_uuid(row.get::<_, String>(2)?)?,
        executor_profile_id: parse_optional_uuid(row.get::<_, Option<String>>(3)?)?,
        goal_id: parse_optional_uuid(row.get::<_, Option<String>>(4)?)?,
        compiled_by: row.get(5)?,
        assigned_role_id: parse_optional_uuid(row.get::<_, Option<String>>(6)?)?,
        effective_executor_kind: row
            .get::<_, Option<String>>(7)?
            .map(|value| ExecutorKind::from_str(&value).map_err(to_sql_conversion_error))
            .transpose()?,
        status: RunStatus::from_str(&row.get::<_, String>(8)?).map_err(to_sql_conversion_error)?,
        requested_by: row.get(9)?,
        created_at: parse_datetime(&row.get::<_, String>(10)?)?,
        updated_at: parse_datetime(&row.get::<_, String>(11)?)?,
    })
}

fn map_agent_role(row: &Row<'_>) -> rusqlite::Result<AgentRole> {
    Ok(AgentRole {
        id: parse_uuid(row.get::<_, String>(0)?)?,
        name: row.get(1)?,
        description: row.get(2)?,
        system_prompt: row.get(3)?,
        default_executor_kind: row
            .get::<_, Option<String>>(4)?
            .map(|value| ExecutorKind::from_str(&value).map_err(to_sql_conversion_error))
            .transpose()?,
        created_at: parse_datetime(&row.get::<_, String>(5)?)?,
        updated_at: parse_datetime(&row.get::<_, String>(6)?)?,
    })
}

fn map_skill(row: &Row<'_>) -> rusqlite::Result<SkillDefinition> {
    Ok(SkillDefinition {
        id: parse_uuid(row.get::<_, String>(0)?)?,
        name: row.get(1)?,
        description: row.get(2)?,
        instructions: row.get(3)?,
        source: SkillSource::from_str(&row.get::<_, String>(4)?).map_err(to_sql_conversion_error)?,
        source_uri: row.get(5)?,
        created_at: parse_datetime(&row.get::<_, String>(6)?)?,
        updated_at: parse_datetime(&row.get::<_, String>(7)?)?,
    })
}

fn map_goal(row: &Row<'_>) -> rusqlite::Result<GoalSpec> {
    Ok(GoalSpec {
        id: parse_uuid(row.get::<_, String>(0)?)?,
        project_id: parse_uuid(row.get::<_, String>(1)?)?,
        kind: GoalKind::from_str(&row.get::<_, String>(2)?).map_err(to_sql_conversion_error)?,
        title: row.get(3)?,
        prompt: row.get(4)?,
        status: GoalStatus::from_str(&row.get::<_, String>(5)?).map_err(to_sql_conversion_error)?,
        compiled_workflow_template_id: parse_optional_uuid(row.get::<_, Option<String>>(6)?)?,
        created_at: parse_datetime(&row.get::<_, String>(7)?)?,
        updated_at: parse_datetime(&row.get::<_, String>(8)?)?,
    })
}

fn map_run_step(row: &Row<'_>) -> rusqlite::Result<RunStep> {
    Ok(RunStep {
        id: parse_uuid(row.get::<_, String>(0)?)?,
        run_id: parse_uuid(row.get::<_, String>(1)?)?,
        workflow_step_id: parse_uuid(row.get::<_, String>(2)?)?,
        status: RunStepStatus::from_str(&row.get::<_, String>(3)?)
            .map_err(to_sql_conversion_error)?,
        external_session_id: row.get(4)?,
        created_at: parse_datetime(&row.get::<_, String>(5)?)?,
        updated_at: parse_datetime(&row.get::<_, String>(6)?)?,
    })
}

fn map_workflow_step(row: &Row<'_>) -> rusqlite::Result<WorkflowStep> {
    let input_schema_json: String = row.get(13)?;
    let output_schema_json: String = row.get(14)?;
    Ok(WorkflowStep {
        id: parse_uuid(row.get::<_, String>(0)?)?,
        workflow_template_id: parse_uuid(row.get::<_, String>(1)?)?,
        name: row.get(2)?,
        instruction: row.get(3)?,
        order_index: row.get(4)?,
        executor_kind: ExecutorKind::from_str(&row.get::<_, String>(5)?)
            .map_err(to_sql_conversion_error)?,
        role_id: parse_optional_uuid(row.get::<_, Option<String>>(6)?)?,
        depends_on_step_id: parse_optional_uuid(row.get::<_, Option<String>>(7)?)?,
        timeout_seconds: row.get(8)?,
        retry_limit: row.get(9)?,
        requires_approval: row.get::<_, i64>(10)? == 1,
        success_criteria: row.get(11)?,
        artifact_contract: row.get(12)?,
        input_schema: serde_json::from_str(&input_schema_json).map_err(to_sql_conversion_error)?,
        output_schema: serde_json::from_str(&output_schema_json).map_err(to_sql_conversion_error)?,
    })
}

fn map_approval_gate(row: &Row<'_>) -> rusqlite::Result<ApprovalGate> {
    Ok(ApprovalGate {
        id: parse_uuid(row.get::<_, String>(0)?)?,
        run_id: parse_uuid(row.get::<_, String>(1)?)?,
        run_step_id: parse_optional_uuid(row.get::<_, Option<String>>(2)?)?,
        status: ApprovalDecision::from_str(&row.get::<_, String>(3)?)
            .map_err(to_sql_conversion_error)?,
        requested_by: row.get(4)?,
        resolved_by: row.get(5)?,
        notes: row.get(6)?,
        created_at: parse_datetime(&row.get::<_, String>(7)?)?,
        resolved_at: parse_optional_datetime(row.get::<_, Option<String>>(8)?)?,
    })
}

fn map_pairing_session(row: &Row<'_>) -> rusqlite::Result<PairingSession> {
    Ok(PairingSession {
        id: parse_uuid(row.get::<_, String>(0)?)?,
        token: row.get(1)?,
        label: row.get(2)?,
        is_revoked: row.get::<_, i64>(3)? == 1,
        created_at: parse_datetime(&row.get::<_, String>(4)?)?,
        expires_at: parse_optional_datetime(row.get::<_, Option<String>>(5)?)?,
    })
}

fn map_artifact(row: &Row<'_>) -> rusqlite::Result<Artifact> {
    let metadata_json: String = row.get(7)?;
    Ok(Artifact {
        id: parse_uuid(row.get::<_, String>(0)?)?,
        run_id: parse_uuid(row.get::<_, String>(1)?)?,
        run_step_id: parse_optional_uuid(row.get::<_, Option<String>>(2)?)?,
        name: row.get(3)?,
        kind: row.get(4)?,
        path: row.get(5)?,
        content_type: row.get(6)?,
        metadata_json: serde_json::from_str(&metadata_json).map_err(to_sql_conversion_error)?,
        created_at: parse_datetime(&row.get::<_, String>(8)?)?,
    })
}

fn parse_optional_uuid(value: Option<String>) -> rusqlite::Result<Option<Uuid>> {
    value.map(parse_uuid).transpose()
}

fn parse_datetime(value: &str) -> rusqlite::Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .map(|datetime| datetime.with_timezone(&Utc))
        .map_err(to_sql_conversion_error)
}

fn parse_optional_datetime(value: Option<String>) -> rusqlite::Result<Option<DateTime<Utc>>> {
    value.as_deref().map(parse_datetime).transpose()
}

fn to_timestamp(value: DateTime<Utc>) -> String {
    value.to_rfc3339()
}

fn to_sql_conversion_error<E>(error: E) -> rusqlite::Error
where
    E: std::fmt::Display + Send + Sync + 'static,
{
    rusqlite::Error::FromSqlConversionFailure(
        0,
        rusqlite::types::Type::Text,
        Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            error.to_string(),
        )),
    )
}

#[cfg(test)]
mod tests {
    use domain::{EventScope, ExecutorKind, NewWorkflowStep};

    use super::OrchestratorStore;

    #[test]
    fn applies_schema_and_lists_projects() {
        let store = OrchestratorStore::open_in_memory().expect("store");
        let projects = store.list_projects().expect("projects");
        assert!(projects.is_empty());
    }

    #[test]
    fn creates_workflow_runs_and_events() {
        let store = OrchestratorStore::open_in_memory().expect("store");

        let project = store
            .create_project(domain::NewProject {
                name: "Example".into(),
                description: Some("Main project".into()),
                workspace_path: "/tmp/example".into(),
                repository_url: Some("https://github.com/example/repo".into()),
                default_executor_profile_id: None,
            })
            .expect("project");

        let executor = store
            .create_executor_profile(domain::NewExecutorProfile {
                name: "nca".into(),
                kind: ExecutorKind::NativeCliAi,
                binary_path: Some("nca".into()),
                config_json: serde_json::json!({"permission_mode": "bypass-permissions"}),
            })
            .expect("executor");

        let workflow = store
            .create_workflow(domain::NewWorkflowTemplate {
                project_id: Some(project.id),
                name: "Repo audit".into(),
                description: Some("Inspect the project".into()),
                steps: vec![NewWorkflowStep {
                    name: "Plan".into(),
                    instruction: "Inspect the repo and draft a plan".into(),
                    order_index: 0,
                    executor_kind: ExecutorKind::NativeCliAi,
                    role_id: None,
                    depends_on_step_id: None,
                    timeout_seconds: Some(120),
                    retry_limit: 1,
                    requires_approval: true,
                    success_criteria: None,
                    artifact_contract: None,
                    input_schema: serde_json::json!({}),
                    output_schema: serde_json::json!({}),
                }],
            })
            .expect("workflow");

        let run = store
            .create_run(domain::NewRun {
                project_id: project.id,
                workflow_template_id: workflow.id,
                executor_profile_id: Some(executor.id),
                goal_id: None,
                compiled_by: None,
                assigned_role_id: None,
                effective_executor_kind: None,
                requested_by: Some("operator".into()),
            })
            .expect("run");

        let gate = store
            .create_approval_gate(run.id, None, Some("operator".into()))
            .expect("gate");

        let event = domain::EventEnvelope::new(
            EventScope::Run,
            "run.created",
            "Run created",
            serde_json::json!({"run_id": run.id}),
        );
        store.record_event(&event).expect("event");

        let workflows = store.list_workflows().expect("workflows");
        let runs = store.list_runs().expect("runs");
        let approvals = store.list_approval_gates().expect("approvals");
        let events = store.list_events().expect("events");

        assert_eq!(workflows.len(), 1);
        assert_eq!(workflows[0].steps.len(), 1);
        assert_eq!(runs.len(), 1);
        assert_eq!(approvals.len(), 1);
        assert_eq!(approvals[0].id, gate.id);
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn creates_and_revokes_pairing_sessions() {
        let store = OrchestratorStore::open_in_memory().expect("store");

        let session = store
            .create_pairing_session(Some("Phone".into()), None)
            .expect("pairing session");
        assert!(store
            .find_active_pairing_session_by_token(&session.token)
            .expect("lookup")
            .is_some());

        let revoked = store
            .revoke_pairing_session(session.id)
            .expect("revoke pairing");
        assert!(revoked.is_revoked);
        assert!(store
            .find_active_pairing_session_by_token(&session.token)
            .expect("lookup")
            .is_none());
    }

    #[test]
    fn persists_artifacts_for_runs() {
        let store = OrchestratorStore::open_in_memory().expect("store");

        let project = store
            .create_project(domain::NewProject {
                name: "Artifacts".into(),
                description: None,
                workspace_path: "/tmp/artifacts".into(),
                repository_url: None,
                default_executor_profile_id: None,
            })
            .expect("project");
        let workflow = store
            .create_workflow(domain::NewWorkflowTemplate {
                project_id: Some(project.id),
                name: "Artifacts flow".into(),
                description: None,
                steps: vec![domain::NewWorkflowStep {
                    name: "Step".into(),
                    instruction: "Do the work".into(),
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
        let run = store
            .create_run(domain::NewRun {
                project_id: project.id,
                workflow_template_id: workflow.id,
                executor_profile_id: None,
                goal_id: None,
                compiled_by: None,
                assigned_role_id: None,
                effective_executor_kind: None,
                requested_by: None,
            })
            .expect("run");

        store
            .create_artifact(
                run.id,
                None,
                "Run summary",
                "summary",
                Some("application/json".into()),
                serde_json::json!({"status": "completed"}),
            )
            .expect("artifact");

        let artifacts = store.list_artifacts_for_run(run.id).expect("artifacts");
        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0].kind, "summary");
    }
}
