# Enterprise Orchestration

An open-source, enterprise-grade orchestration platform for running and supervising AI workflows from a fast, local-first desktop app.

## Vision

This project is building a **Tauri-first orchestration app** inspired by:

- Conductor / Maestro style multi-agent supervision
- Dify / CrewAI / Haystack workflow concepts
- Mission Control style operator dashboards

The product is designed around:

- a **Tauri desktop shell**
- an embedded **Rust control server**
- a shared **React control UI**
- a **CLI executor adapter** model with `native-cli-ai` as the primary integration
- responsive browser control so operators can approve or monitor runs from a phone

## Current status

The repository is in active greenfield development.

The current milestone focuses on:

1. workspace bootstrap,
2. core Rust crates,
3. local persistence,
4. a local control server,
5. executor adapters,
6. Tauri shell and shared control UI.

## Architecture at a glance

```text
Tauri Desktop Shell
  ├─ React control UI
  ├─ Rust orchestration core
  ├─ Embedded Axum control server
  ├─ SQLite persistence
  ├─ Executor adapters
  └─ Phone/browser remote control
```

## Monorepo layout

```text
apps/
  control-ui/   React operator UI
  desktop/      Tauri shell

crates/
  domain/       shared domain types
  persistence/  SQLite schema + repositories
  orchestrator/ workflow execution engine
  runtime/      process/worktree/session runtime
  executors/    CLI executor adapters
  control-server/ local HTTP + realtime APIs
  desktop-core/ Tauri integration layer
  security/     tokens, secrets, pairing
  observability/ event fanout and logs
```

## Tooling

- Rust workspace via `cargo`
- frontend workspace via `pnpm`
- React + Vite for the control UI
- Axum for the control server

## Local development

### Prerequisites

- Rust 1.83+
- Node.js 22+
- pnpm 10+

### Install frontend dependencies

```bash
pnpm install
```

### Build the control UI

```bash
pnpm build
```

### Check the Rust workspace

```bash
cargo test
```

## Environment

Copy the example environment file if needed:

```bash
cp .env.example .env
```

Important settings:

- `ORCH_CONTROL_BIND`
- `ORCH_CONTROL_PORT`
- `ORCH_DB_PATH`
- `NCA_BINARY`

## Primary executor strategy

The first-class executor target is [`native-cli-ai`](https://github.com/madebyaris/native-cli-ai), integrated through its public orchestration contract.

Planned secondary adapters:

- Claude Code
- Codex CLI
- OpenCode
- shell/script execution

## License

Apache-2.0
