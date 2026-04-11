use std::process::{Command, Stdio};

use anyhow::{anyhow, Result};
use domain::{EventEnvelope, EventScope, ExecutorKind};
use serde_json::json;

use crate::adapter::{ExecutorAdapter, ExecutorCapability, ExecutorRunRequest, ExecutorSession};

const SHELL_CAPABILITIES: &[ExecutorCapability] = &[ExecutorCapability::Cancellation];

#[derive(Debug, Clone, Default)]
pub struct ShellExecutorAdapter;

impl ExecutorAdapter for ShellExecutorAdapter {
    fn kind(&self) -> ExecutorKind {
        ExecutorKind::Shell
    }

    fn display_name(&self) -> &'static str {
        "shell"
    }

    fn capabilities(&self) -> &'static [ExecutorCapability] {
        SHELL_CAPABILITIES
    }

    fn binary_name(&self) -> &'static str {
        "sh"
    }

    fn start_run(
        &self,
        request: &ExecutorRunRequest,
        on_event: &mut dyn FnMut(EventEnvelope),
    ) -> Result<ExecutorSession> {
        on_event(EventEnvelope::new(
            EventScope::Run,
            "executor.shell.started",
            "Shell command started",
            json!({"command": request.prompt}),
        ));

        let binary = request.binary_path.as_deref().unwrap_or("sh");
        let mut command = Command::new(binary);
        command.args(["-lc", request.prompt.as_str()]);
        if let Some(workspace_path) = &request.workspace_path {
            command.current_dir(workspace_path);
        }
        for (key, value) in &request.orchestration_env {
            command.env(key, value);
        }

        let output = command.output()?;
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

        on_event(EventEnvelope::new(
            EventScope::Run,
            "executor.shell.completed",
            "Shell command completed",
            json!({
                "status": output.status.code(),
                "stdout": stdout,
                "stderr": stderr,
            }),
        ));

        if !output.status.success() {
            return Err(anyhow!("shell command exited with {}", output.status));
        }

        Ok(ExecutorSession {
            session_id: None,
            pid: None,
            raw: json!({
                "stdout": String::from_utf8_lossy(&output.stdout),
                "stderr": String::from_utf8_lossy(&output.stderr),
            }),
        })
    }

    fn spawn_session(&self, request: &ExecutorRunRequest) -> Result<ExecutorSession> {
        let binary = request.binary_path.as_deref().unwrap_or("sh");
        let mut command = Command::new(binary);
        command.args(["-lc", request.prompt.as_str()]);
        command.stdout(Stdio::null()).stderr(Stdio::null());
        if let Some(workspace_path) = &request.workspace_path {
            command.current_dir(workspace_path);
        }
        for (key, value) in &request.orchestration_env {
            command.env(key, value);
        }

        let child = command.spawn()?;
        Ok(ExecutorSession {
            session_id: Some(child.id().to_string()),
            pid: Some(child.id()),
            raw: json!({"pid": child.id()}),
        })
    }

    fn cancel(&self, session_id: &str) -> Result<serde_json::Value> {
        let output = Command::new("kill").arg(session_id).output()?;
        if !output.status.success() {
            return Err(anyhow!("failed to cancel pid {session_id}"));
        }

        Ok(json!({"cancelled": true, "pid": session_id}))
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        adapter::{ExecutorAdapter, ExecutorRunRequest},
        shell::ShellExecutorAdapter,
    };

    #[test]
    fn executes_shell_commands_and_emits_events() {
        let adapter = ShellExecutorAdapter;
        let mut events = Vec::new();

        let session = adapter
            .start_run(
                &ExecutorRunRequest {
                    prompt: "printf hello".into(),
                    workspace_path: None,
                    permission_mode: None,
                    binary_path: None,
                    config_json: serde_json::json!({}),
                    orchestration_env: Vec::new(),
                },
                &mut |event| events.push(event),
            )
            .expect("shell run");

        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event_type, "executor.shell.started");
        assert_eq!(events[1].payload["stdout"], "hello");
        assert!(session.raw["stdout"]
            .as_str()
            .unwrap_or_default()
            .contains("hello"));
    }
}
