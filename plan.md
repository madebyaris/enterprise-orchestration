# Enterprise Orchestration - Project Plan

## Overview

An open-source, enterprise-grade orchestration platform for running and supervising AI workflows from a fast, local-first desktop app built with Tauri.

## Project Status

| Component | Status | Notes |
|-----------|--------|-------|
| Monorepo structure | ✅ Done | pnpm workspace + Cargo workspace |
| Rust workspace | ✅ Done | 9 crates compiled |
| Control UI (React) | ✅ Done | Builds successfully |
| Desktop shell (Tauri) | 🔄 Pending | `pnpm dev:desktop` not tested |
| Cargo tests | ⚠️ 6/7 | 1 test fails (hardcoded CI path) |

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

| Crate | Purpose |
|-------|---------|
| `domain` | Shared domain types |
| `persistence` | SQLite schema + repositories |
| `orchestrator` | Workflow execution engine |
| `runtime` | Process/worktree/session runtime |
| `executors` | CLI executor adapters |
| `control-server` | Local HTTP + realtime APIs (Axum) |
| `desktop-core` | Tauri integration layer |
| `security` | Tokens, secrets, pairing |
| `observability` | Event fanout and logs |

## Apps

| App | Purpose |
|-----|---------|
| `apps/desktop` | Tauri shell |
| `apps/control-ui` | React operator UI |

## Current Issues

### 1. Failing Test
- **Test:** `serves_frontend_index_when_dist_is_configured`
- **Issue:** Hardcoded CI path `/workspace/apps/control-ui/dist`
- **Fix needed:** Use dynamic path or env var for `frontend_dist`

### 2. Missing Environment Setup
- `.env.example` exists but `.env` not created
- Key vars: `ORCH_CONTROL_BIND`, `ORCH_CONTROL_PORT`, `ORCH_DB_PATH`, `NCA_BINARY`

## Roadmap

### Phase 1: Local Development ✅
- [x] Install dependencies (`pnpm install`)
- [x] Build control UI (`pnpm build`)
- [x] Verify Rust compilation (`cargo build --workspace`)
- [x] Run tests (`cargo test --workspace`)
- [ ] Fix failing test (hardcoded path)
- [ ] Test desktop dev server (`pnpm dev:desktop`)

### Phase 2: Core Functionality
- [ ] Implement workflow execution in `orchestrator`
- [ ] Complete `executors` adapters (native-cli-ai first)
- [ ] Build control server API endpoints
- [ ] Wire up SQLite persistence

### Phase 3: UI Development
- [ ] Build React control UI components
- [ ] Connect UI to control server
- [ ] Implement project/workflow/run management
- [ ] Add approval gate UI

### Phase 4: Desktop Integration
- [ ] Tauri window management
- [ ] System tray integration
- [ ] Desktop notifications
- [ ] Remote control pairing flow

### Phase 5: Polish & Release
- [ ] CI/CD setup
- [ ] Documentation
- [ ] Packaging (`.dmg`, `.exe`, `.AppImage`)
- [ ] Release process

## Next Steps

### Immediate (Today)
1. ~~Create `.env` from `.env.example`~~ ✅ Done
2. Fix the failing test in `control-server`
3. Run `pnpm dev:desktop` to verify desktop shell works
4. Explore the orchestrator crate to understand workflow execution

### This Week
1. Complete control server API implementation
2. Implement persistence layer
3. Start building React UI
4. Test end-to-end workflow creation

## Commands Reference

```bash
# Install dependencies
pnpm install

# Build control UI
pnpm build

# Run Rust tests
cargo test --workspace

# Development
pnpm dev:desktop    # Desktop app
pnpm dev:ui         # Control UI only

# Rust tools
cargo check         # Type check
cargo clippy        # Linting
cargo fmt           # Formatting
```

## Resources

- [Tauri v2 Docs](https://tauri.app/)
- [React 19](https://react.dev/)
- [Axum](https://docs.rs/axum/latest/axum/)
- [SQLite + rusqlite](https://docs.rs/rusqlite/latest/rusqlite/)
