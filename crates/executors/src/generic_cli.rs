use std::process::{Command, Stdio};

use anyhow::{anyhow, Result};
use domain::{EventEnvelope, EventScope};
use serde_json::{json, Value};

use crate::adapter::{ExecutorCapability, ExecutorRunRequest, ExecutorSession};

pub const GENERIC_CLI_CAPABILITIES: &[ExecutorCapability] = &[
    ExecutorCapability::BackgroundSessions,
    ExecutorCapability::Cancellation,
];

pub fn start_run(
    display_name: &str,
    binary_name: &str,
    request: &ExecutorRunRequest,
    default_run_template: &[&str],
    on_event: &mut dyn FnMut(EventEnvelope),
) -> Result<ExecutorSession> {
    let binary = request.binary_path.as_deref().unwrap_or(binary_name);
    let args = render_template(
        request,
        request
            .config_json
            .get("run_template")
            .and_then(as_string_array)
            .unwrap_or_else(|| default_run_template.iter().map(|value| value.to_string()).collect()),
        None,
    );

    on_event(EventEnvelope::new(
        EventScope::Run,
        format!("executor.{}.started", slug(display_name)),
        format!("{display_name} command started"),
        json!({ "binary": binary, "args": args }),
    ));

    let mut command = base_command(binary, request);
    command.args(&args);
    let output = command.output()?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

    on_event(EventEnvelope::new(
        EventScope::Run,
        format!("executor.{}.completed", slug(display_name)),
        format!("{display_name} command completed"),
        json!({
            "binary": binary,
            "args": args,
            "status": output.status.code(),
            "stdout": stdout,
            "stderr": stderr,
        }),
    ));

    if !output.status.success() {
        return Err(anyhow!("{display_name} exited with {}", output.status));
    }

    Ok(ExecutorSession {
        session_id: None,
        pid: None,
        raw: json!({
            "binary": binary,
            "args": args,
            "stdout": String::from_utf8_lossy(&output.stdout),
            "stderr": String::from_utf8_lossy(&output.stderr),
        }),
    })
}

pub fn spawn_session(
    binary_name: &str,
    request: &ExecutorRunRequest,
    default_spawn_template: &[&str],
) -> Result<ExecutorSession> {
    let binary = request.binary_path.as_deref().unwrap_or(binary_name);
    let args = render_template(
        request,
        request
            .config_json
            .get("spawn_template")
            .and_then(as_string_array)
            .unwrap_or_else(|| default_spawn_template.iter().map(|value| value.to_string()).collect()),
        None,
    );

    let mut command = base_command(binary, request);
    command.args(&args);
    command.stdout(Stdio::null()).stderr(Stdio::null());

    let child = command.spawn()?;
    Ok(ExecutorSession {
        session_id: Some(child.id().to_string()),
        pid: Some(child.id()),
        raw: json!({
            "binary": binary,
            "args": args,
            "pid": child.id(),
        }),
    })
}

pub fn cancel(binary_name: &str, cancel_template: &[&str], session_id: &str) -> Result<Value> {
    if !cancel_template.is_empty() {
        let args = cancel_template
            .iter()
            .map(|value| value.replace("{session_id}", session_id))
            .collect::<Vec<_>>();
        let output = Command::new(binary_name).args(&args).output()?;
        if !output.status.success() {
            return Err(anyhow!("{binary_name} cancel exited with {}", output.status));
        }
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        return Ok(json!({
            "cancelled": true,
            "session_id": session_id,
            "stdout": stdout,
            "stderr": stderr,
        }));
    }

    let output = Command::new("kill").arg(session_id).output()?;
    if !output.status.success() {
        return Err(anyhow!("failed to cancel pid {session_id}"));
    }

    Ok(json!({"cancelled": true, "pid": session_id}))
}

fn base_command(binary: &str, request: &ExecutorRunRequest) -> Command {
    let mut command = Command::new(binary);
    if let Some(workspace_path) = &request.workspace_path {
        command.current_dir(workspace_path);
    }
    for (key, value) in &request.orchestration_env {
        command.env(key, value);
    }
    command
}

fn render_template(
    request: &ExecutorRunRequest,
    template: Vec<String>,
    session_id: Option<&str>,
) -> Vec<String> {
    template
        .into_iter()
        .map(|value| {
            let value = value.replace("{prompt}", &request.prompt);
            match session_id {
                Some(session_id) => value.replace("{session_id}", session_id),
                None => value,
            }
        })
        .collect()
}

fn as_string_array(value: &Value) -> Option<Vec<String>> {
    value
        .as_array()
        .map(|items| items.iter().filter_map(|item| item.as_str().map(str::to_owned)).collect())
}

fn slug(display_name: &str) -> String {
    display_name
        .to_ascii_lowercase()
        .replace(' ', "_")
        .replace('-', "_")
}
