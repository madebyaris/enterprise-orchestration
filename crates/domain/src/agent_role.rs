use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::executor::ExecutorKind;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRole {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub system_prompt: String,
    pub default_executor_kind: Option<ExecutorKind>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewAgentRole {
    pub name: String,
    pub description: Option<String>,
    pub system_prompt: String,
    pub default_executor_kind: Option<ExecutorKind>,
}
