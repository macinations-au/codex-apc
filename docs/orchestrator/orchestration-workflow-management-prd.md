# Orchestration Workflow Management — PRD

## Summary

Introduce a CLI “orchestrator” that executes structured work plans as a task graph (DAG). The orchestrator consumes a machine‑readable plan (YAML/JSON) describing tasks, shards (subtasks), dependencies, acceptance criteria, approvals, and commands. It coordinates end‑to‑end flows: plan → reason → edit → build/test → review → deploy → document, with audit logs and a human‑readable status dashboard.

## Goals

- Deterministic execution of a master task list (and shards) with dependency order (DAG).
- Fast agent handoff: provide minimal, precise context so the agent can act without back‑and‑forth.
- Clear acceptance gates (tests, file presence, content checks) for automated success/failure.
- Reproducible runs with JSONL event logs and resumability across failures/approvals.
- Human oversight via a generated `TASKS.md` status dashboard.
- Index‑first navigation to locate relevant code, minimizing ad‑hoc grep.

## Non‑Goals

- Replace general project management tools; scope is repo‑local execution.
- Heavy workflow engines or external schedulers. Keep it simple and local.

## Users & Scenarios

- Maintainer running a multi‑step feature: generate code, add tests, build, review, and tag.
- Contributor proposing refactors guarded by lint/tests and approval gates.
- Release manager cutting a release with validation, artifacts, and docs updates.

## High‑Level Architecture

- Orchestrator subcommand: `codex-agentic orchestrate …`.
- Plan spec (YAML/JSON) defining a task DAG with shards and acceptance.
- Execution engine that runs tasks, emits events, and writes status.
- Event log: JSONL in `.codex/runs/<timestamp>.jsonl`.
- Status mirror: `TASKS.md` with live progress, links, and outcomes.
- Approvals & gates for sensitive actions (e.g., deploy, apply_patch, tag).
- Build & install step mirrors CI (release build and local sync for testing).
- Index‑first navigation for discovery before edits.

## CLI Spec (Initial)

```bash
# Validate plan against schema
codex-agentic orchestrate plan validate [--file orchestrator.yml]

# Execute tasks (DAG‑aware)
codex-agentic orchestrate run [--file orchestrator.yml] [--task <ID>|--all] \
  [--resume] [--dry-run] [--no-build]

# Show current progress/state
codex-agentic orchestrate status [--file orchestrator.yml]

# Resume after a failure or approval gate
codex-agentic orchestrate resume [--file orchestrator.yml]

# Approve a named gate (e.g., deploy)
codex-agentic orchestrate approve <gate> [--file orchestrator.yml]

# Generate a run report (summary + links + diffs)
codex-agentic orchestrate report [--file orchestrator.yml] [--out REPORT.md]
```

Flags
- `--file`: path to plan (`orchestrator.yml` default).
- `--task`: run a specific task and its dependencies.
- `--all`: run entire DAG.
- `--resume`: continue from last recorded state.
- `--dry-run`: evaluate plan and print intended actions without executing.
- `--no-build`: skip the final release build/install phase.

## Plan Spec (v1)

Canonical: YAML (easier to author). JSON accepted equivalently. The orchestrator keeps YAML as source of truth and can generate `TASKS.md` for humans.

Minimal fields per task
- `id`, `title`, `description`
- `type`: code_change | docs | test | release | deploy | chore
- `paths`: hints for files/dirs to edit/create
- `commands`: lint/test/build/release commands
- `acceptance`: concrete checks (tests, file/content assertions)
- `dependencies`: task IDs
- `shards`: list of sub‑tasks with their own `id/title/acceptance`
- `constraints`: style, language, forbidden paths, timebox
- `approvals`: human gates (e.g., `apply_patch`, `deploy`)

Project metadata (optional)
- `project`: name, root, language, build defaults
- `orchestrator`: strategy, global approvals/gates

Example (YAML)
```yaml
version: 1
project:
  name: codex-agentic
  root: .
  language: rust
  build:
    lint: "cargo clippy -- -D warnings"
    test: "cargo test"
    release: "cargo build --release"
orchestrator:
  strategy: conservative
  approvals:
    required: ["apply_patch", "deploy"]

tasks:
  - id: EXC-001
    title: Central exception handling
    type: code_change
    description: Add a central error type and mapping so CLI exits with consistent codes/messages.
    paths:
      preferred:
        - "codex-agentic/src/error.rs"
        - "codex-agentic/src/lib.rs"
    commands:
      lint: "cargo clippy -- -D warnings"
      test: "cargo test --package codex-agentic"
      build: "cargo build --release -p codex-agentic"
    acceptance:
      - "Tests pass: cargo test --package codex-agentic"
      - "File exists: codex-agentic/src/error.rs"
      - "Contains: enum AppError"
    dependencies: []
    shards:
      - id: EXC-001A
        title: Define AppError enum
        acceptance:
          - "Contains: impl std::error::Error for AppError"
      - id: EXC-001B
        title: Map anyhow::Error to AppError
```

JSON variant: equivalent keys and structure; the orchestrator supports both.

## File Conventions

- Plan file: `orchestrator.yml` at repo root. Large plans may split under `tasks/` and be included by the main file.
- Dashboard: orchestrator generates/updates `TASKS.md` at repo root.
- Logs: `.codex/runs/<timestamp>.jsonl` (append‑only event stream).

## Event Model

Event types
- `plan_validated`, `plan_updated`, `task_started`, `task_completed`, `task_failed`, `shard_started`, `shard_completed`, `acceptance_passed`, `acceptance_failed`, `command_run`, `patch_applied`, `awaiting_approval`, `resumed`, `skipped`, `completed`.

Event format (JSONL)
```json
{ "ts": "2025-09-22T11:03:14Z", "type": "task_started", "task": "EXC-001", "shard": null }
{ "ts": "2025-09-22T11:03:20Z", "type": "command_run", "task": "EXC-001", "cmd": "cargo test", "status": 0 }
{ "ts": "2025-09-22T11:03:28Z", "type": "acceptance_passed", "task": "EXC-001", "check": "Tests pass" }
{ "ts": "2025-09-22T11:03:30Z", "type": "task_completed", "task": "EXC-001" }
```

## Interaction Model (What the Agent Needs)

- Code path hints: where to edit/create files and which crates/modules to touch.
- Language/framework + versions (Rust edition, Node, Python, etc.).
- Style/conventions: naming, layout rules, prior patterns.
- Build/test commands: exact commands to validate quickly.
- Acceptance criteria: tests, command exit codes, file diffs or text patterns.
- Interfaces/usage: how new code is invoked; example inputs/outputs.
- Constraints: performance/security constraints, forbidden directories, public API stability.
- Deployment target: what “done” means (docs updated, binary installed, CI green, release tagged).

For example, “create a class to handle exceptions” (Rust)
- Where: `codex-agentic/src/error.rs` (new), wire in `codex-agentic/src/lib.rs`.
- Shape: `enum AppError { … }` with `From<anyhow::Error>`, `Display`, and exit‑code mapping.
- Tests: `codex-agentic/tests/error.rs` covering mapping and messages.
- Acceptance: tests pass; binary maps known errors to consistent exit codes.

## Execution Flow

1) Discover: use index‑first search to find relevant files/patterns.
2) Plan: expand tasks into shards, sequence by dependencies.
3) Edit: apply minimal patches guided by `paths` hints.
4) Validate: run lint/tests; iterate on failures.
5) Summarize: update `TASKS.md` and write events.
6) Gate: pause on approvals; resume on approval.
7) Release: run release build and sync local install for verification.

State values
- `pending`, `in_progress`, `blocked`, `failed`, `completed`, `skipped`.

## Safety & Approvals

- Gates: `approvals.required` at plan or task level (e.g., `apply_patch`, `deploy`, `tag`).
- Sensitive operations (destructive edits, deploys, tagging) require explicit approval via `approve <gate>`.
- Dry‑run mode prints intended actions without executing.

## Telemetry & Logs

- JSONL events with timestamps and exit codes.
- Summarized reports with links to diffs, artifacts, and CI runs.
- Run IDs for cross‑referencing CI/PRs.

## Failure Modes & Recovery

- Network/transient failures: retry policy with backoff for fetch/build; re‑run capability.
- Dependency failures: mark downstream tasks `blocked` until resolved.
- Acceptance failures: capture logs and diffs, stay ready to `resume` after fixes.

## CI Integration

- Mirror local behavior in CI: validate plan, dry‑run, optional execution for safe tasks.
- Release workflow uses the same `release` commands defined in the plan.

## Build & Install (Local Mirror)

- After task runs that change code, run a release build and sync to `~/.cargo/bin` (per repo scripts) for quick testing.
- Commands (defaults, overridable in plan):

```bash
(cd codex-agentic && cargo build --release)
scripts/codex-agentic.sh --help >/dev/null 2>&1 || true  # syncs installed binary
```

## Open Questions

- Should the orchestrator auto‑generate skeleton plans from prompts?
- How much schema enforcement vs. free‑form acceptance commands?
- Versioned plan schema migrations and compatibility policy.

## Appendix A — Minimal `orchestrator.yml`

```yaml
version: 1
project:
  name: codex-agentic
  root: .
  language: rust
orchestrator:
  approvals:
    required: []

tasks:
  - id: DOC-001
    title: Add orchestration PRD
    type: docs
    description: Create docs/orchestrator/orchestration-workflow-management-prd.md
    paths:
      preferred:
        - docs/orchestrator/
    commands: {}
    acceptance:
      - "File exists: docs/orchestrator/orchestration-workflow-management-prd.md"
    dependencies: []
```

## Appendix B — `TASKS.md` (Generated)

```markdown
# Tasks — Status Dashboard

- [x] DOC-001 — Add orchestration PRD (completed)
- [ ] EXC-001 — Central exception handling (pending)
```

## Summary

This PRD specifies a practical, local‑first orchestration system for codex‑agentic: tasks are modeled as a DAG with shards and explicit acceptance criteria; the CLI orchestrator executes them deterministically with logs and human oversight. Plans are defined in YAML/JSON, progress is mirrored to `TASKS.md`, and builds/install mirror CI for quick validation.

