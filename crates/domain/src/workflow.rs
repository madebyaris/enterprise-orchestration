use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::executor::ExecutorKind;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowTemplate {
    pub id: Uuid,
    pub project_id: Option<Uuid>,
    pub name: String,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub steps: Vec<WorkflowStep>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowStep {
    pub id: Uuid,
    pub workflow_template_id: Uuid,
    pub name: String,
    pub instruction: String,
    pub order_index: i32,
    pub executor_kind: ExecutorKind,
    pub role_id: Option<Uuid>,
    pub depends_on_step_id: Option<Uuid>,
    pub timeout_seconds: Option<i64>,
    pub retry_limit: i32,
    pub requires_approval: bool,
    pub success_criteria: Option<String>,
    pub artifact_contract: Option<String>,
    #[serde(default)]
    pub input_schema: serde_json::Value,
    #[serde(default)]
    pub output_schema: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewWorkflowTemplate {
    pub project_id: Option<Uuid>,
    pub name: String,
    pub description: Option<String>,
    pub steps: Vec<NewWorkflowStep>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewWorkflowStep {
    pub name: String,
    pub instruction: String,
    pub order_index: i32,
    pub executor_kind: ExecutorKind,
    #[serde(default)]
    pub role_id: Option<Uuid>,
    #[serde(default)]
    pub depends_on_step_id: Option<Uuid>,
    #[serde(default)]
    pub timeout_seconds: Option<i64>,
    #[serde(default)]
    pub retry_limit: i32,
    #[serde(default)]
    pub requires_approval: bool,
    #[serde(default)]
    pub success_criteria: Option<String>,
    #[serde(default)]
    pub artifact_contract: Option<String>,
    #[serde(default)]
    pub input_schema: serde_json::Value,
    #[serde(default)]
    pub output_schema: serde_json::Value,
}
