# Architecture Overview

## Product shape

Enterprise Orchestration is a **desktop-first, local-first orchestration platform**.

The desktop app is the control plane:

- it stores local state,
- supervises runs,
- exposes a local HTTP control server,
- renders a shared control UI inside Tauri,
- optionally serves the same UI to a paired phone browser.

## Core layers

### 1. Desktop shell
- Tauri shell
- native windowing and platform integration
- secure command boundary into Rust services

### 2. Control UI
- React + TypeScript
- Mission Control style operator views
- works both inside the Tauri webview and in a normal browser

### 3. Control server
- Axum-based local API server
- REST endpoints for projects, workflows, runs, approvals, and settings
- SSE/WebSocket event streaming for realtime updates

### 4. Orchestration core
- run state machine
- step dependency tracking
- approval gates
- durable event generation
- restart recovery

### 5. Persistence
- SQLite for local durable state
- schema migrations checked into the repository
- append-only event history plus structured tables

### 6. Executor adapters
- `native-cli-ai` as primary supported adapter
- secondary adapters for Claude Code, Codex CLI, OpenCode, and shell execution
- shared internal event model hides executor-specific differences from the UI

## Remote control model

The desktop app can expose a local-network control surface.

Planned flow:

1. user enables remote access,
2. app generates a one-time pairing token,
3. app displays a QR code,
4. phone browser opens the responsive control UI,
5. operator can monitor runs and handle approvals from the phone.

## Design constraints

- fast and RAM-friendly
- robust local persistence
- OSS-friendly setup
- enterprise-grade operational UX
- avoid early over-investment in canvas/RAG features
