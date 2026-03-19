pub mod adapter;
pub mod claude_code;
pub mod codex;
pub mod native_cli_ai;
pub mod opencode;
pub mod shell;

pub use adapter::{
    ExecutorAdapter, ExecutorCapability, ExecutorHealth, ExecutorRunRequest, ExecutorSession,
};
pub use claude_code::ClaudeCodeAdapter;
pub use codex::CodexAdapter;
pub use native_cli_ai::{parse_ndjson_line, parse_ndjson_stream, NativeCliAiAdapter};
pub use opencode::OpenCodeAdapter;
pub use shell::ShellExecutorAdapter;

pub fn default_health_checks() -> Vec<ExecutorHealth> {
    vec![
        NativeCliAiAdapter::default().detect(),
        ClaudeCodeAdapter::default().detect(),
        CodexAdapter::default().detect(),
        OpenCodeAdapter::default().detect(),
        ShellExecutorAdapter::default().detect(),
    ]
}

#[cfg(test)]
mod tests {
    use domain::ExecutorKind;

    use crate::default_health_checks;

    #[test]
    fn includes_all_executor_health_checks() {
        let health_checks = default_health_checks();
        assert_eq!(health_checks.len(), 5);
        assert!(health_checks
            .iter()
            .any(|health| health.kind == ExecutorKind::NativeCliAi));
        assert!(health_checks
            .iter()
            .any(|health| health.kind == ExecutorKind::Shell));
    }
}
