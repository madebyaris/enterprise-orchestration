use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillSource {
    Inline,
    AgentsMd,
    File,
    Remote,
}

impl SkillSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Inline => "inline",
            Self::AgentsMd => "agents_md",
            Self::File => "file",
            Self::Remote => "remote",
        }
    }
}

impl FromStr for SkillSource {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "inline" => Ok(Self::Inline),
            "agents_md" => Ok(Self::AgentsMd),
            "file" => Ok(Self::File),
            "remote" => Ok(Self::Remote),
            _ => Err(format!("unknown skill source: {value}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillDefinition {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub instructions: String,
    pub source: SkillSource,
    pub source_uri: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewSkillDefinition {
    pub name: String,
    pub description: Option<String>,
    pub instructions: String,
    pub source: SkillSource,
    pub source_uri: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillBinding {
    pub role_id: Uuid,
    pub skill_id: Uuid,
    pub created_at: DateTime<Utc>,
}
