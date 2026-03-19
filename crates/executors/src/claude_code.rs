use anyhow::{anyhow, Result};
use domain::ExecutorKind;

use crate::adapter::{ExecutorAdapter, ExecutorCapability, ExecutorRunRequest, ExecutorSession};

#[derive(Debug, Clone, Default)]
pub struct ClaudeCodeAdapter;

impl ExecutorAdapter for ClaudeCodeAdapter {
    fn kind(&self) -> ExecutorKind {
        ExecutorKind::ClaudeCode
    }

    fn display_name(&self) -> &'static str {
        "Claude Code"
    }

    fn capabilities(&self) -> &'static [ExecutorCapability] {
        &[]
    }

    fn binary_name(&self) -> &'static str {
        "claude"
    }

    fn start_run(
        &self,
        _request: &ExecutorRunRequest,
        _on_event: &mut dyn FnMut(domain::EventEnvelope),
    ) -> Result<ExecutorSession> {
        Err(anyhow!(
            "Claude Code adapter scaffolding is present but subprocess integration is not implemented yet"
        ))
    }

    fn spawn_session(&self, _request: &ExecutorRunRequest) -> Result<ExecutorSession> {
        Err(anyhow!(
            "Claude Code adapter scaffolding is present but subprocess integration is not implemented yet"
        ))
    }

    fn cancel(&self, _session_id: &str) -> Result<serde_json::Value> {
        Err(anyhow!(
            "Claude Code adapter scaffolding is present but subprocess integration is not implemented yet"
        ))
    }
}
