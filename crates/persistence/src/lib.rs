use std::{
    path::Path,
    str::FromStr,
    sync::{Arc, Mutex, MutexGuard},
};

use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use domain::{
    ApprovalDecision, ApprovalGate, EventEnvelope, EventScope, ExecutorKind, ExecutorProfile,
    NewExecutorProfile, NewProject, NewRun, NewWorkflowTemplate, Project, Run, RunStatus, RunStep,
    RunStepStatus, WorkflowStep, WorkflowTemplate,
};
use rusqlite::{params, Connection, OptionalExtension, Row};
use uuid::Uuid;

const INITIAL_MIGRATION: &str = include_str!("../migrations/0001_initial.sql");

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
            archived_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO projects
            (id, name, description, workspace_path, repository_url, default_executor_profile_id, archived_at, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                project.id.to_string(),
                &project.name,
                project.description.as_deref(),
                &project.workspace_path,
                project.repository_url.as_deref(),
                project.default_executor_profile_id.map(|value| value.to_string()),
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
            "SELECT id, name, description, workspace_path, repository_url, default_executor_profile_id, archived_at, created_at, updated_at
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
                archived_at: parse_optional_datetime(row.get::<_, Option<String>>(6)?)?,
                created_at: parse_datetime(&row.get::<_, String>(7)?)?,
                updated_at: parse_datetime(&row.get::<_, String>(8)?)?,
            })
        })?;

        rows.collect::<rusqlite::Result<Vec<_>>>()
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
                    depends_on_step_id: step.depends_on_step_id,
                    timeout_seconds: step.timeout_seconds,
                    retry_limit: step.retry_limit,
                    requires_approval: step.requires_approval,
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
                (id, workflow_template_id, name, instruction, order_index, executor_kind, depends_on_step_id, timeout_seconds, retry_limit, requires_approval)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    step.id.to_string(),
                    workflow.id.to_string(),
                    &step.name,
                    &step.instruction,
                    step.order_index,
                    step.executor_kind.as_str(),
                    step.depends_on_step_id.map(|value| value.to_string()),
                    step.timeout_seconds,
                    step.retry_limit,
                    step.requires_approval as i64,
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
            (id, project_id, workflow_template_id, executor_profile_id, status, requested_by, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                run.id.to_string(),
                run.project_id.to_string(),
                run.workflow_template_id.to_string(),
                run.executor_profile_id.map(|value| value.to_string()),
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
            "SELECT id, project_id, workflow_template_id, executor_profile_id, status, requested_by, created_at, updated_at
             FROM runs
             ORDER BY created_at DESC",
        )?;

        let rows = statement.query_map([], |row| {
            Ok(Run {
                id: parse_uuid(row.get::<_, String>(0)?)?,
                project_id: parse_uuid(row.get::<_, String>(1)?)?,
                workflow_template_id: parse_uuid(row.get::<_, String>(2)?)?,
                executor_profile_id: parse_optional_uuid(row.get::<_, Option<String>>(3)?)?,
                status: RunStatus::from_str(&row.get::<_, String>(4)?)
                    .map_err(to_sql_conversion_error)?,
                requested_by: row.get(5)?,
                created_at: parse_datetime(&row.get::<_, String>(6)?)?,
                updated_at: parse_datetime(&row.get::<_, String>(7)?)?,
            })
        })?;

        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn get_run(&self, run_id: Uuid) -> Result<Option<Run>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, project_id, workflow_template_id, executor_profile_id, status, requested_by, created_at, updated_at
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
            "SELECT id, workflow_template_id, name, instruction, order_index, executor_kind, depends_on_step_id, timeout_seconds, retry_limit, requires_approval
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
            "SELECT id, workflow_template_id, name, instruction, order_index, executor_kind, depends_on_step_id, timeout_seconds, retry_limit, requires_approval
             FROM workflow_steps
             WHERE workflow_template_id = ?1
             ORDER BY order_index ASC",
        )?;

        let rows = statement.query_map([workflow_template_id.to_string()], map_workflow_step)?;

        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
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
        status: RunStatus::from_str(&row.get::<_, String>(4)?).map_err(to_sql_conversion_error)?,
        requested_by: row.get(5)?,
        created_at: parse_datetime(&row.get::<_, String>(6)?)?,
        updated_at: parse_datetime(&row.get::<_, String>(7)?)?,
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
    Ok(WorkflowStep {
        id: parse_uuid(row.get::<_, String>(0)?)?,
        workflow_template_id: parse_uuid(row.get::<_, String>(1)?)?,
        name: row.get(2)?,
        instruction: row.get(3)?,
        order_index: row.get(4)?,
        executor_kind: ExecutorKind::from_str(&row.get::<_, String>(5)?)
            .map_err(to_sql_conversion_error)?,
        depends_on_step_id: parse_optional_uuid(row.get::<_, Option<String>>(6)?)?,
        timeout_seconds: row.get(7)?,
        retry_limit: row.get(8)?,
        requires_approval: row.get::<_, i64>(9)? == 1,
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
                    depends_on_step_id: None,
                    timeout_seconds: Some(120),
                    retry_limit: 1,
                    requires_approval: true,
                }],
            })
            .expect("workflow");

        let run = store
            .create_run(domain::NewRun {
                project_id: project.id,
                workflow_template_id: workflow.id,
                executor_profile_id: Some(executor.id),
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
}
