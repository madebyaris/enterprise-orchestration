use std::{
    io::{BufRead, BufReader},
    process::{Command, Stdio},
};

use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use domain::{EventEnvelope, EventScope, ExecutorKind};
use serde::Deserialize;
use serde_json::Value;

use crate::adapter::{
    resolve_binary, ExecutorAdapter, ExecutorCapability, ExecutorHealth, ExecutorRunRequest,
    ExecutorSession,
};

const NATIVE_CLI_AI_CAPABILITIES: &[ExecutorCapability] = &[
    ExecutorCapability::StructuredStreaming,
    ExecutorCapability::BackgroundSessions,
    ExecutorCapability::SessionAttach,
    ExecutorCapability::Cancellation,
    ExecutorCapability::ApprovalAware,
    ExecutorCapability::CostTracking,
    ExecutorCapability::WorktreeAware,
];

#[derive(Debug, Clone)]
pub struct NativeCliAiAdapter {
    pub binary: String,
}

impl Default for NativeCliAiAdapter {
    fn default() -> Self {
        Self {
            binary: std::env::var("NCA_BINARY").unwrap_or_else(|_| "nca".into()),
        }
    }
}

impl ExecutorAdapter for NativeCliAiAdapter {
    fn kind(&self) -> ExecutorKind {
        ExecutorKind::NativeCliAi
    }

    fn display_name(&self) -> &'static str {
        "native-cli-ai"
    }

    fn capabilities(&self) -> &'static [ExecutorCapability] {
        NATIVE_CLI_AI_CAPABILITIES
    }

    fn binary_name(&self) -> &'static str {
        "nca"
    }

    fn detect(&self) -> ExecutorHealth {
        let binary_path = resolve_binary(&self.binary);
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
    ) -> Result<ExecutorSession> {
        let mut command = self.base_command(request);
        command.args([
            "run",
            "--stream",
            "ndjson",
            "--prompt",
            request.prompt.as_str(),
            "--permission-mode",
            request.permission_mode.as_deref().unwrap_or("dont-ask"),
        ]);
        command.stdout(Stdio::piped()).stderr(Stdio::piped());

        let mut child = command
            .spawn()
            .with_context(|| format!("failed to spawn {}", self.binary))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("missing stdout for {}", self.binary))?;

        let reader = BufReader::new(stdout);
        let mut session_id = None;
        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }

            let event = parse_ndjson_line(&line)?;
            if session_id.is_none() {
                session_id = event
                    .payload
                    .get("session_id")
                    .and_then(|value| value.as_str())
                    .map(ToOwned::to_owned);
            }
            on_event(event);
        }

        let output = child.wait_with_output()?;
        if !output.status.success() {
            return Err(anyhow!(
                "{} run exited with status {}",
                self.binary,
                output.status
            ));
        }

        Ok(ExecutorSession {
            session_id,
            pid: None,
            raw: serde_json::json!({
                "stdout": String::from_utf8_lossy(&output.stdout),
                "stderr": String::from_utf8_lossy(&output.stderr),
            }),
        })
    }

    fn spawn_session(&self, request: &ExecutorRunRequest) -> Result<ExecutorSession> {
        let mut command = self.base_command(request);
        command.args([
            "spawn",
            "--prompt",
            request.prompt.as_str(),
            "--json",
            "--permission-mode",
            request.permission_mode.as_deref().unwrap_or("dont-ask"),
        ]);

        let output = command
            .output()
            .with_context(|| format!("failed to spawn {}", self.binary))?;
        if !output.status.success() {
            return Err(anyhow!(
                "{} spawn exited with status {}",
                self.binary,
                output.status
            ));
        }

        let raw: Value = serde_json::from_slice(&output.stdout)?;
        Ok(ExecutorSession {
            session_id: raw
                .get("session_id")
                .and_then(|value| value.as_str())
                .map(ToOwned::to_owned),
            pid: raw
                .get("pid")
                .and_then(|value| value.as_u64())
                .and_then(|value| u32::try_from(value).ok()),
            raw,
        })
    }

    fn cancel(&self, session_id: &str) -> Result<Value> {
        let output = Command::new(&self.binary)
            .args(["cancel", session_id, "--json"])
            .output()
            .with_context(|| format!("failed to cancel session via {}", self.binary))?;
        if !output.status.success() {
            return Err(anyhow!(
                "{} cancel exited with status {}",
                self.binary,
                output.status
            ));
        }

        Ok(serde_json::from_slice(&output.stdout)?)
    }
}

impl NativeCliAiAdapter {
    fn base_command(&self, request: &ExecutorRunRequest) -> Command {
        let binary = request.binary_path.as_deref().unwrap_or(&self.binary);
        let mut command = Command::new(binary);
        if let Some(workspace_path) = &request.workspace_path {
            command.current_dir(workspace_path);
        }

        for (key, value) in &request.orchestration_env {
            command.env(key, value);
        }

        command
    }
}

#[derive(Debug, Deserialize)]
struct NcaStreamEnvelope {
    ts: Option<String>,
    event: Value,
}

pub fn parse_ndjson_stream(input: &str) -> Result<Vec<EventEnvelope>> {
    input
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(parse_ndjson_line)
        .collect()
}

pub fn parse_ndjson_line(line: &str) -> Result<EventEnvelope> {
    let envelope: NcaStreamEnvelope = serde_json::from_str(line)?;
    let event_type = envelope
        .event
        .get("type")
        .and_then(|value| value.as_str())
        .map(camel_to_snake)
        .unwrap_or_else(|| "unknown".into());

    let created_at = envelope
        .ts
        .as_deref()
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&Utc))
        .unwrap_or_else(Utc::now);

    Ok(EventEnvelope {
        id: uuid::Uuid::new_v4(),
        scope: EventScope::Run,
        event_type: format!("executor.native_cli_ai.{event_type}"),
        summary: format!("native-cli-ai event {}", envelope_type_summary(&event_type)),
        payload: envelope.event,
        created_at,
    })
}

fn envelope_type_summary(event_type: &str) -> String {
    event_type.replace('_', " ")
}

fn camel_to_snake(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for (index, character) in value.chars().enumerate() {
        if character.is_uppercase() {
            if index > 0 {
                output.push('_');
            }
            for lowered in character.to_lowercase() {
                output.push(lowered);
            }
        } else {
            output.push(character);
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::{camel_to_snake, parse_ndjson_line, parse_ndjson_stream};

    #[test]
    fn converts_camel_case_to_snake_case() {
        assert_eq!(camel_to_snake("ToolCallStarted"), "tool_call_started");
        assert_eq!(camel_to_snake("SessionEnded"), "session_ended");
    }

    #[test]
    fn parses_native_cli_ai_event_lines() {
        let event = parse_ndjson_line(
            r#"{"id":12,"ts":"2026-03-14T08:00:00Z","event":{"type":"ToolCallStarted","call_id":"call_123","tool":"read_file","input":{"path":"src/main.rs"}}}"#,
        )
        .expect("event");

        assert_eq!(event.event_type, "executor.native_cli_ai.tool_call_started");
        assert_eq!(event.payload["tool"], "read_file");
    }

    #[test]
    fn parses_multi_line_event_streams() {
        let events = parse_ndjson_stream(
            r#"{"id":1,"ts":"2026-03-14T08:00:00Z","event":{"type":"SessionStarted","session_id":"session-123"}}
{"id":2,"ts":"2026-03-14T08:00:02Z","event":{"type":"SessionEnded","session_id":"session-123"}}"#,
        )
        .expect("events");

        assert_eq!(events.len(), 2);
        assert_eq!(events[0].payload["session_id"], "session-123");
        assert_eq!(events[1].event_type, "executor.native_cli_ai.session_ended");
    }
}
