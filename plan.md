# Enterprise Orchestration - Project Plan

## Overview

An open-source, enterprise-grade orchestration platform for running and supervising AI workflows from a fast, local-first desktop app built with Tauri.

## Project Status


| Component             | Status | Notes                            |
| --------------------- | ------ | -------------------------------- |
| Monorepo structure    | ✅ Done | pnpm workspace + Cargo workspace |
| Rust workspace        | ✅ Done | 9 crates, all compile            |
| Control UI (React)    | ✅ Done | Builds successfully              |
| Desktop shell (Tauri) | ✅ Done | Dev server runs                  |
| Cargo tests           | ✅ Done | **25/25 tests pass**             |


## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    Tauri Desktop Shell                      │
├─────────────┬─────────────┬─────────────┬──────────────────┤
│ React UI    │ Orchestrator│ SQLite      │ Executor Adapters│
│ (control-ui)│ (workflow)  │ (persistence)│ (CLI integration)│
├─────────────┴─────────────┴─────────────┴──────────────────┤
│              Embedded Axum Control Server                   │
├─────────────────────────────────────────────────────────────┤
│              Rust Core Crates                               │
│  domain | runtime | executors | control-server | security  │
│  observability | desktop-core                              │
└─────────────────────────────────────────────────────────────┘
```

## Crates


| Crate            | Purpose                           |
| ---------------- | --------------------------------- |
| `domain`         | Shared domain types               |
| `persistence`    | SQLite schema + repositories      |
| `orchestrator`   | Workflow execution engine         |
| `runtime`        | Process/worktree/session runtime  |
| `executors`      | CLI executor adapters             |
| `control-server` | Local HTTP + realtime APIs (Axum) |
| `desktop-core`   | Tauri integration layer           |
| `security`       | Tokens, secrets, pairing          |
| `observability`  | Event fanout and logs             |


## Orchestrator Crate - Deep Dive

### Key Components

`**RunOrchestrator**` (`run_orchestrator.rs`)

- Main workflow state machine
- `start_run()` - Creates a new run and drives it
- `complete_running_step()` - Marks step complete and drives next
- `fail_running_step()` - Marks step failed, stops run
- `approve_gate()` / `reject_gate()` - Handle approval gates
- `recover_in_progress_runs()` - Resume runs after restart
- `drive_run()` - Core state machine logic

`**step_dispatcher.rs**`

- `terminal_run_status()` - Determines if run is complete/failed/cancelled
- `next_runnable_step()` - Finds next step that can run (respects dependencies)
- `has_running_step()` / `has_waiting_approval()` - Status checks

`**approval_service.rs**`

- `request_for_step()` - Creates approval gate for a step
- `approve()` / `reject()` - Resolves approval gates

### Run State Machine

```
RunStatus: Queued → Running → WaitingForApproval ↔ Running → Completed
                ↓
              Failed
                ↓
            Cancelled
```

### Workflow Execution Model

1. Create workflow template with ordered steps
2. Steps have `depends_on_step_id` for dependencies
3. Steps can have `requires_approval: true` to pause for human input
4. Executor handles actual execution (shell, native-cli-ai, etc.)

### Tests (4 passing)

- `sequential_workflow_executes_in_order` - Step ordering
- `approval_gate_blocks_and_resumes` - Approval flow
- `recovers_run_state_after_restart` - Persistence
- `respects_attempt_limit` - Retry policy

## Apps


| App               | Purpose           |
| ----------------- | ----------------- |
| `apps/desktop`    | Tauri shell       |
| `apps/control-ui` | React operator UI |


## Completed Items

### Phase 1: Local Development ✅

- Install dependencies (`pnpm install`)
- Build control UI (`pnpm build`)
- Verify Rust compilation (`cargo build --workspace`)
- Run tests (`cargo test --workspace`) - **25/25 pass**
- Fix failing test (hardcoded path) - Uses `CARGO_MANIFEST_DIR`
- Test desktop dev server (`pnpm dev:desktop`) - Works

### Additional Setup

- Install MiniMax M2 cursor rules
- Install skills: find-skills, rust-refactor, tauri-development
- Create `.env` from `.env.example`
- Add `plan.md` with roadmap

## Current Issues

### None - All Phase 1 items complete

## Phase 2 Progress: Executor-Orchestrator Integration

### Executor Driver (`executor_driver.rs`)

Created `crates/orchestrator/src/executor_driver.rs` to bridge orchestrator state machine with executor adapters.

**Key Components:**

- `ExecutorDriver` - Polls for Running steps and drives execution
- `drive_step()` - Executes a single step via the appropriate adapter
- `poll_and_drive_ready_steps()` - Scans all runs, executes any Running steps
- Integrates with `ShellExecutorAdapter` and `NativeCliAiAdapter`

**How It Works:**

```
Orchestrator (state machine)          ExecutorDriver (bridge)
┌─────────────────────────┐           ┌─────────────────────────┐
│ marks step "Running"    │──────────▶│ poll_and_drive_ready_    │
│                         │           │   steps()                │
│                         │           │                          │
│                         │           │ selects adapter by kind  │
│                         │           │ calls adapter.start_run()│
│                         │           │                          │
│                         │◀──────────│ completes step           │
│ receives completion     │           │                          │
└─────────────────────────┘           └─────────────────────────┘
```

**Integration Point:**

The `DesktopRuntime` now owns an `ExecutorDriver` that runs in a background task. Every 500ms it polls for Running steps and executes them.

**Added to DesktopRuntime:**
- `start_executor_driver()` - Starts background polling task
- `stop_executor_driver()` - Graceful shutdown

**New Methods in Persistence:**
- `get_project()` - Retrieve project by ID
- `get_executor_profile()` - Retrieve executor profile by ID
- `update_run_step_external_session()` - Update session ID after executor runs
- `execute()` - Helper for raw SQL execution

**Domain Changes:**
- `ExecutorKind` now derives `Hash` for use in HashMap

## Roadmap

### Phase 1: Local Development ✅

COMPLETE - Desktop dev server running, all 25 tests pass

### Phase 2: Core Functionality

- **Implement executor execution** - ✅ ExecutorDriver bridge created
- **Wire ExecutorDriver into DesktopRuntime** - ✅ Background task polling every 500ms
- **Add missing persistence methods** - ✅ get_project, get_executor_profile, update_run_step_external_session
- Complete `runtime` crate integration - Pending (still a stub)

### Phase 3: UI Development

- Build React control UI components
- Connect UI to control server API
- Implement project/workflow/run management views
- Add approval gate UI (approve/reject buttons)

### Phase 4: Desktop Integration

- Tauri window management
- System tray integration
- Desktop notifications for approvals
- Remote control pairing flow

### Phase 5: Polish & Release

- CI/CD setup
- Documentation
- Packaging (`.dmg`, `.exe`, `.AppImage`)
- Release process

## Next Steps

### Immediate

1. **Implement executor execution** - ✅ ExecutorDriver created and wired into DesktopRuntime
2. **Wire ExecutorDriver into DesktopRuntime** - ✅ Background task polls every 500ms
3. **Complete runtime crate** - Still a placeholder, needs worktree/session management

### This Week

1. Get end-to-end: start run → executor runs → step completes → run progresses
2. Build UI for creating/viewing workflows
3. Add approval notifications in desktop

## Commands Reference

```bash
# Install dependencies
pnpm install

# Build control UI
pnpm build

# Run Rust tests
cargo test --workspace

# Development
pnpm dev:desktop    # Desktop app (currently running)
pnpm dev:ui         # Control UI only

# Rust tools
cargo check         # Type check
cargo clippy        # Linting
cargo fmt           # Formatting
```

## Test Results

```
control-server:  7/7 passed ✅
desktop-core:   2/2 passed ✅
executors:      5/5 passed ✅
orchestrator:   4/4 passed ✅
persistence:    4/4 passed ✅
runtime:        1/1 passed ✅
security:       1/1 passed ✅
observability:  1/1 passed ✅
───────────────────────────────
TOTAL:         25/25 passed ✅
```

## Resources

- [Tauri v2 Docs](https://tauri.app/)
- [React 19](https://react.dev/)
- [Axum](https://docs.rs/axum/latest/axum/)
- [SQLite + rusqlite](https://docs.rs/rusqlite/latest/rusqlite/)

