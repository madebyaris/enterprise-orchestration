use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub workspace_path: String,
    pub repository_url: Option<String>,
    pub default_executor_profile_id: Option<Uuid>,
    pub agents_md_path: Option<String>,
    pub agents_md_updated_at: Option<DateTime<Utc>>,
    pub archived_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewProject {
    pub name: String,
    pub description: Option<String>,
    pub workspace_path: String,
    pub repository_url: Option<String>,
    pub default_executor_profile_id: Option<Uuid>,
}
