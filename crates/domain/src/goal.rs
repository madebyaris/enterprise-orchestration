use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalKind {
    CreateApp,
    CreateWorkflow,
}

impl GoalKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::CreateApp => "create_app",
            Self::CreateWorkflow => "create_workflow",
        }
    }
}

impl FromStr for GoalKind {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "create_app" => Ok(Self::CreateApp),
            "create_workflow" => Ok(Self::CreateWorkflow),
            _ => Err(format!("unknown goal kind: {value}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalStatus {
    Draft,
    Compiled,
    Running,
    Completed,
    Failed,
}

impl GoalStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Compiled => "compiled",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }
}

impl FromStr for GoalStatus {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "draft" => Ok(Self::Draft),
            "compiled" => Ok(Self::Compiled),
            "running" => Ok(Self::Running),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            _ => Err(format!("unknown goal status: {value}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoalSpec {
    pub id: Uuid,
    pub project_id: Uuid,
    pub kind: GoalKind,
    pub title: String,
    pub prompt: String,
    pub status: GoalStatus,
    pub compiled_workflow_template_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewGoalSpec {
    pub project_id: Uuid,
    pub kind: GoalKind,
    pub title: String,
    pub prompt: String,
}
