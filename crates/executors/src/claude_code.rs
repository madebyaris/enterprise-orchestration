use anyhow::Result;
use domain::ExecutorKind;

use crate::adapter::{ExecutorAdapter, ExecutorCapability, ExecutorRunRequest, ExecutorSession};
use crate::generic_cli::{self, GENERIC_CLI_CAPABILITIES};

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
        GENERIC_CLI_CAPABILITIES
    }

    fn binary_name(&self) -> &'static str {
        "claude"
    }

    fn start_run(
        &self,
        request: &ExecutorRunRequest,
        on_event: &mut dyn FnMut(domain::EventEnvelope),
    ) -> Result<ExecutorSession> {
        generic_cli::start_run("claude_code", self.binary_name(), request, &["-p", "{prompt}"], on_event)
    }

    fn spawn_session(&self, request: &ExecutorRunRequest) -> Result<ExecutorSession> {
        generic_cli::spawn_session(self.binary_name(), request, &["-p", "{prompt}"])
    }

    fn cancel(&self, session_id: &str) -> Result<serde_json::Value> {
        generic_cli::cancel(self.binary_name(), &[], session_id)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::{ExecutorAdapter, ExecutorRunRequest};

    use super::ClaudeCodeAdapter;

    #[test]
    fn executes_with_configured_template_override() {
        let adapter = ClaudeCodeAdapter;
        let mut events = Vec::new();
        let session = adapter
            .start_run(
                &ExecutorRunRequest {
                    prompt: "ignored".into(),
                    workspace_path: None,
                    permission_mode: None,
                    binary_path: Some("sh".into()),
                    config_json: json!({
                        "run_template": ["-lc", "printf claude"]
                    }),
                    orchestration_env: Vec::new(),
                },
                &mut |event| events.push(event),
            )
            .expect("run");

        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event_type, "executor.claude_code.started");
        assert_eq!(session.raw["stdout"], "claude");
    }
}
