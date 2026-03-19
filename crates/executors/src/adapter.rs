use std::path::Path;

use anyhow::Result;
use domain::{EventEnvelope, ExecutorKind};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutorCapability {
    StructuredStreaming,
    BackgroundSessions,
    SessionAttach,
    Cancellation,
    ApprovalAware,
    CostTracking,
    WorktreeAware,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutorHealth {
    pub kind: ExecutorKind,
    pub available: bool,
    pub binary_path: Option<String>,
    pub version_hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutorRunRequest {
    pub prompt: String,
    pub workspace_path: Option<String>,
    pub permission_mode: Option<String>,
    #[serde(default)]
    pub orchestration_env: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutorSession {
    pub session_id: Option<String>,
    pub pid: Option<u32>,
    pub raw: serde_json::Value,
}

pub trait ExecutorAdapter {
    fn kind(&self) -> ExecutorKind;
    fn display_name(&self) -> &'static str;
    fn capabilities(&self) -> &'static [ExecutorCapability];
    fn binary_name(&self) -> &'static str;

    fn detect(&self) -> ExecutorHealth {
        let binary_path = resolve_binary(self.binary_name());
        ExecutorHealth {
            kind: self.kind(),
            available: binary_path.is_some(),
            binary_path,
            version_hint: None,
        }
    }

    fn start_run(
        &self,
        request: &ExecutorRunRequest,
        on_event: &mut dyn FnMut(EventEnvelope),
    ) -> Result<ExecutorSession>;

    fn spawn_session(&self, request: &ExecutorRunRequest) -> Result<ExecutorSession>;

    fn cancel(&self, session_id: &str) -> Result<serde_json::Value>;
}

pub fn resolve_binary(binary: &str) -> Option<String> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|entry| entry.join(binary))
        .find(|candidate| candidate.is_file())
        .map(path_to_string)
}

pub fn path_to_string(path: impl AsRef<Path>) -> String {
    path.as_ref().to_string_lossy().into_owned()
}
