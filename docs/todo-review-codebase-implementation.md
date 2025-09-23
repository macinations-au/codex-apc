# Implementation Task: `/about-codebase` (TUI)

## Overview
Build a new TUI slash command, `/about-codebase`, that generates a structured, persistent “Codebase Review” for the current workspace. It scans the repo (git‑aware), reads and samples key files locally (no model‑side exec), prompts the model to produce a Markdown report, saves the result to `${cwd}/.codex/review-codebase.json`, and shows live progress while working. Subsequent runs perform a delta update using Git and per‑file SHA‑256.

Primary design reference: `docs/enh-review-codebase.md`.

## Goals
- Produce a high‑quality Markdown review (architecture, flows, CI/Release, config/env, design choices, risks).
- Persist a JSON report with metadata (git snapshot, file hashes, inputs hash, model info, token usage).
- Delta updates via `git diff --name-status <commit_hash>..HEAD` and per‑file `sha256` (fallback when not in Git).
- Live progress in the transcript during scanning and generation (MUST‑HAVE).

## Non‑Goals (MVP)
- ACP parity; deep multi‑pass crawling across very large monorepos (a future `--deep` mode can add this).

## Deliverables
1) Slash command `/about-codebase [--refresh|-r]` wired into the TUI.
2) New module `codex-tui/src/review_codebase.rs` with scan, hash, sample, prompt, and persist logic.
3) JSON schema saved to `${cwd}/.codex/review-codebase.json`.
4) Unit/integration tests and brief documentation updates.

## Key References (repo paths)
- Design doc: `docs/enh-review-codebase.md`
- Slash command enum: `codex-tui/src/slash_command.rs`
- TUI dispatch (command handling): `codex-tui/src/chatwidget.rs` (see `dispatch_command`, ~lines 900–1000)
- History cells for status: `codex-tui/src/history_cell.rs` (e.g., `new_review_status_line`, `new_info_event`, `new_warning_event`)
- Git helpers pattern: `codex-tui/src/get_git_diff.rs` (Tokio process usage)

## Implementation Steps

### 1) Add the new slash command
- File: `codex-tui/src/slash_command.rs`
- Add enum variant and description (kebab-case):
```rust
#[derive(...)]
#[strum(serialize_all = "kebab-case")]
pub enum SlashCommand {
    // …
    AboutCodebase,
}

impl SlashCommand {
    pub fn description(self) -> &'static str { match self { /* … */
        SlashCommand::AboutCodebase => "Tell me about this codebase (usage: /about-codebase [--refresh|-r])",
        // …
    }}
    pub fn available_during_task(self) -> bool { match self { /* … */
        SlashCommand::AboutCodebase => false,
        // …
    }}
}
```

### 2) Create `review_codebase.rs`
- File: `codex-tui/src/review_codebase.rs`
- Public API (proposed):
```rust
pub struct ReviewInputs {
    pub commit_hash: Option<String>,
    pub context_source: &'static str, // "git" | "filesystem"
    pub files: Vec<FileEntry>,        // curated + changed
    pub inputs_hash: String,
}

pub struct FileEntry {
    pub path: std::path::PathBuf,
    pub size_bytes: u64,
    pub modified_at: Option<chrono::DateTime<chrono::Utc>>,
    pub sha256: String,
    pub binary: bool,
    pub sampled_text: Option<String>, // sampled/truncated (for prompt)
}

pub struct JsonReport { /* matches docs/enh-review-codebase.md */ }

pub async fn run_review_codebase(
    app_tx: crate::app_event_sender::AppEventSender,
    config: codex_core::config::Config,
    previous_report: Option<JsonReport>,
    force: bool,
) -> anyhow::Result<()> {
    // 1) announce start (progress line)
    // 2) collect context (git snapshot, curated files, deltas)
    // 3) rate-limited read + sample + hash + inputs_hash
    // 4) build prompt (initial or update includes prior Markdown)
    // 5) send prompt via Op::UserInput (reuse existing streaming UI)
    // 6) await turn completion and then atomic-save JsonReport
    // 7) post a status line: "Saved to .codex/review-codebase.json"
}
```

Notes:
- Rate limit: 10 file reads/sec. Implementation idea: `tokio::time::interval` + `.tick().await` around reads.
- Binary detection: check magic bytes (e.g., `infer` crate) and/or simple UTF‑8 check.
- Sampling policy: for large text, concatenate top/middle/bottom slices with a banner: `--- sampled N of M bytes ---`.
- Git snapshot: `git rev-parse HEAD`, `git status --porcelain` to set `is_dirty`; deltas via `git diff --name-status <commit>..HEAD`.
- Fallback when not in Git: detect with `rev-parse --is-inside-work-tree`; then rely on `sha256/size` changes.
- Atomic save: write to `${cwd}/.codex/review-codebase.json.tmp`, then `rename` to final path.

### 3) Wire command dispatch
- File: `codex-tui/src/chatwidget.rs`
- In `dispatch_command`, add:
```rust
SlashCommand::AboutCodebase => {
    // Show immediate progress line
    self.add_to_history(history_cell::new_review_status_line(
        "Starting codebase review…".to_string(),
    ));
    let app_tx = self.app_event_tx.clone();
    let config = self.config.clone();
    // (Optional) Load previous report JSON if exists
    let prev = load_previous_report(&config)
        .ok();
    tokio::spawn(async move {
        let _ = crate::review_codebase::run_review_codebase(app_tx, config, prev).await;
    });
}

// Typed command support:
// Users can enter `/about-codebase --refresh` directly. The chat widget intercepts
// typed submissions starting with `/about-codebase` and parses flags (`--refresh`|`-r`).
```
- Progress updates from `run_review_codebase` should use `app_tx.send(AppEvent::InsertHistoryCell(history_cell::new_review_status_line(...)))`.

### 4) Prompt assembly
- Initial run: embed sampled contents of curated files; follow template in `docs/enh-review-codebase.md` (“Embedded Contents”).
- Update run: embed full previous Markdown + sampled contents for changed/new files; list removed files.
- Keep a total prompt budget (e.g., ~200 KB). If exceeded, fallback to full review with curated set and sampling.

### 5) Persistence format
- JSON schema in `docs/enh-review-codebase.md`:
  - Root fields: `schema`, `generated_at`, `workspace_root`, `git { branch, head, is_dirty, commit_hash }`, `context_source`, `model`, `token_usage`, `inputs_hash`, `inputs[]`, `references[]`, `report.markdown`.
- Path: `${cwd}/.codex/review-codebase.json` (create `.codex` if missing; atomic write).

### 6) Error handling & recovery
- Git absent or not a repo: set `context_source = "filesystem"`; fall back to hashing/size.
- Corrupted JSON: ignore and re‑generate; log a `new_warning_event` with a brief explanation.
- Partial saves: always write atomically; if a temp file is found on startup, remove it.
- Symlinks/submodules: resolve in‑tree symlinks; record and skip submodules by default.

### 7) Tests
- Unit:
  - Parse `git diff --name-status` output (A/M/D).
  - Hashing + `inputs_hash` calculation.
  - Binary detection; sampling policy (size thresholds).
  - JSON (de)serialization + corrupted JSON recovery.
- Integration:
  - Initial flow generates JSON and shows progress lines.
  - Update flow with a changed file; JSON updates and includes prior Markdown.
- Default: show previous saved report (if available). If stale (>24h) or changes detected, display the report first and then suggest updating. Force update via `/about-codebase --refresh`.

## Acceptance Criteria
- `/about-codebase` appears in the TUI command list with the described help text.
- Running it:
  - Shows progress while scanning (at least 2–3 status lines over time).
  - Produces a Markdown review rendered in the transcript.
  - Writes `${cwd}/.codex/review-codebase.json` using the documented schema.
- Subsequent runs:
  - Use `commit_hash` + `git diff` to include only changed/new files; removed files are listed.
  - If no changes (inputs_hash matches), prints a friendly no‑op message and does not call the model.
- Tests pass (unit + integration path described above).

## Out of Scope (for this task)
- ACP command parity.
- Deep analysis/graph‑based importance scores (leave hooks in place for future).

## Build & Run
```bash
# Build + tests (from repo root)
(cd codex-agentic && cargo fmt --all && cargo clippy -- -D warnings && cargo test && cargo build --release)

# Run the TUI and invoke the command
codex-agentic
# then type: /about-codebase
```

## Notes
- Keep changes minimal and focused (see `AGENTS.md`).
- Follow the existing Tokio + async style (`get_git_diff.rs` shows good patterns for `tokio::process::Command`).
- Use minimal new dependencies: `sha2` for hashing; a small binary‑detection crate if needed (or lightweight magic‑byte checks).

## Summary
This task adds a secure, deterministic `/about-codebase` flow with live progress, Git‑aware deltas, robust hashing, and an auditable JSON artifact, per the design in `docs/enh-review-codebase.md`.
