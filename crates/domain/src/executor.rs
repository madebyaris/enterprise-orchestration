use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutorKind {
    NativeCliAi,
    ClaudeCode,
    Codex,
    OpenCode,
    Shell,
}

impl ExecutorKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NativeCliAi => "native_cli_ai",
            Self::ClaudeCode => "claude_code",
            Self::Codex => "codex",
            Self::OpenCode => "opencode",
            Self::Shell => "shell",
        }
    }
}

impl FromStr for ExecutorKind {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "native_cli_ai" => Ok(Self::NativeCliAi),
            "claude_code" => Ok(Self::ClaudeCode),
            "codex" => Ok(Self::Codex),
            "opencode" => Ok(Self::OpenCode),
            "shell" => Ok(Self::Shell),
            _ => Err(format!("unknown executor kind: {value}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutorProfile {
    pub id: Uuid,
    pub name: String,
    pub kind: ExecutorKind,
    pub binary_path: Option<String>,
    pub config_json: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewExecutorProfile {
    pub name: String,
    pub kind: ExecutorKind,
    pub binary_path: Option<String>,
    #[serde(default)]
    pub config_json: serde_json::Value,
}
