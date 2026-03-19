use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventScope {
    System,
    Project,
    Workflow,
    Run,
    Approval,
    Executor,
}

impl EventScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Project => "project",
            Self::Workflow => "workflow",
            Self::Run => "run",
            Self::Approval => "approval",
            Self::Executor => "executor",
        }
    }
}

impl FromStr for EventScope {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "system" => Ok(Self::System),
            "project" => Ok(Self::Project),
            "workflow" => Ok(Self::Workflow),
            "run" => Ok(Self::Run),
            "approval" => Ok(Self::Approval),
            "executor" => Ok(Self::Executor),
            _ => Err(format!("unknown event scope: {value}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub id: Uuid,
    pub scope: EventScope,
    pub event_type: String,
    pub summary: String,
    pub payload: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

impl EventEnvelope {
    pub fn new(
        scope: EventScope,
        event_type: impl Into<String>,
        summary: impl Into<String>,
        payload: serde_json::Value,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            scope,
            event_type: event_type.into(),
            summary: summary.into(),
            payload,
            created_at: Utc::now(),
        }
    }
}
