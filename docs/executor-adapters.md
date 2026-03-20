# Executor Adapters

The platform uses a CLI adapter layer instead of coupling orchestration directly to a single model provider.

## Primary adapter

### `native-cli-ai`

`native-cli-ai` is the first-class executor integration because it already exposes an orchestration-friendly subprocess contract:

- `run --stream ndjson`
- `spawn --json`
- `status --json`
- `sessions --json`
- `cancel --json`

The adapter in this repository:

- detects the configured `nca` binary,
- parses NDJSON event streams into internal event envelopes,
- parses spawned session metadata,
- normalizes cancellation output.

## Secondary adapters

Scaffolding is present for:

- Claude Code
- Codex CLI
- OpenCode

These adapters currently expose capability metadata and binary detection, but their subprocess wiring is intentionally deferred until the primary `native-cli-ai` flow is battle-tested.

## Shell adapter

The shell adapter exists for:

- non-agent steps,
- simple bootstrap tasks,
- deterministic local smoke testing.

It executes `sh -lc <command>` and emits normalized start/completion events.

## Capability model

Adapters advertise capabilities such as:

- structured streaming
- background sessions
- attach/resume support
- cancellation
- approval awareness
- cost tracking
- worktree awareness

This keeps the rest of the platform from assuming every CLI behaves like `native-cli-ai`.
