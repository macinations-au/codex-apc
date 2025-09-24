codex-apc
================

[![MSRV 1.89+](https://img.shields.io/badge/MSRV-1.89%2B-blue.svg)](codex-acp/rust-toolchain.toml)
[![Rust Edition 2024](https://img.shields.io/badge/Edition-2024-blueviolet.svg)](https://doc.rust-lang.org/edition-guide/rust-2024/index.html)
![CI](https://img.shields.io/github/actions/workflow/status/macinations-au/codex-apc/ci.yml?label=CI)

An Agent Client Protocol (ACP) agent that bridges the OpenAI Codex runtime to ACP‑capable clients (e.g., Zed). On session start it prints a Markdown banner with the application version and status. A minimal set of custom slash commands is built in, and custom prompts are discovered and exposed as commands automatically.

<img width="2414" height="1354" alt="Untitled" src="https://github.com/user-attachments/assets/40199fd6-eebd-41b0-a73b-eb6bbfa6d406" />



Contents
- `codex-acp/` — ACP agent library (used by the launcher for `--acp`)
- `codex-agentic/` — Single binary launcher (CLI by default; `--acp` for ACP)
- `scripts/` — helper scripts (`install.sh`)

Install
-------

- One‑liner (installs latest codex-agentic):
  - `bash <(curl -fsSL https://raw.githubusercontent.com/macinations-au/codex-apc/main/scripts/install.sh)`
- From source (local build):
  - `cargo install --path codex-agentic --force`

Run
---

- `codex-agentic` provides a TUI by default and an ACP (editor) mode:

```bash
# TUI (terminal UI, default)
codex-agentic

# ACP (Agent Client Protocol) server for IDEs
codex-agentic acp
```

Key features in this distribution
- Two modes: TUI (terminal UI) and ACP (editor/stdio server for IDEs like Zed).
- Reasoning view control for both modes: `--reasoning hidden|summary|raw`.
- CLI model picker shows local Ollama models; model picks can persist, provider never does.
- No default heavy model is auto‑pulled in OSS mode.
 - TUI shows compact index status and supports `/index` and `/search` alongside ACP.

Examples
- Minimal CLI session
  - `codex-agentic`
  - Type `/model` → pick an OpenAI or Ollama model
  - Type `/status` to confirm

- Use Ollama models (no flags needed after first pick)
  - `codex-agentic`
  - `/model` → choose `qwq:latest`
  - Provider switches to `oss` and the choice is saved; future runs reuse it

- Collapse “thinking”
  - `codex-agentic --reasoning summary`
  - In the transcript, “Thinking” shows with a chevron (▶). Press `r` to expand/collapse.

- ACP server for IDEs (e.g., Zed)
  - `codex-agentic --acp --reasoning summary`
  - In the IDE, use `/reasoning hidden|summary|raw` to change mid‑session

Zed Integration
---------------

Example (two entries: repo launcher and the installed binary):

```json
"agent_servers": {
  "Codex": {
    "command": "/Users/<you>/workspace/codex-apc/scripts/codex-agentic.sh",
    "env": { "RUST_LOG": "info" }
  },
  "CodexExec": {
    "command": "/Users/<you>/.cargo/bin/codex-agentic",
    "env": { "RUST_LOG": "info" }
  }
}
```

Slash commands (high‑use)
-------------------------
- `/model` — open the model picker; includes local Ollama models; saves model only (never provider)
- `/status` — workspace/account/model/tokens
- `/approvals <policy>` — `untrusted | on-request | on-failure | never`
- `/reasoning <hidden|summary|raw>` — collapse or show “thinking”
- `/init` — scaffold an AGENTS.md in the workspace

Indexing (Local Search)
-----------------------

- Available by default (automatic opt‑in). On first run in a repo, a best‑effort background build prepares a local semantic index used for code search and retrieval.
- Opt‑out from the command line (this run only):

```bash
# Disable indexing for a CLI/TUI run
CODEX_INDEXING=0 codex-agentic

# Disable indexing for ACP (editor) mode
CODEX_INDEXING=0 codex-agentic acp
```

- Where it lives: `.codex/index/` at your project root, containing `manifest.json`, `vectors.hnsw`, `meta.jsonl`, and `analytics.json`.
- Related controls:
  - Disable retrieval injection only: `CODEX_INDEX_RETRIEVAL=0`.
  - Adjust retrieval confidence gate: `CODEX_INDEX_RETRIEVAL_THRESHOLD=0.725` (default).
  - Post‑turn refresh backoff: `CODEX_INDEX_REFRESH_MIN_SECS=300` (default).

Helpful commands

```bash
# Status / Build / Query / Clean
codex-agentic index status
codex-agentic index build [--model bge-small|bge-large]
codex-agentic index query "<text>" -k 8 --show-snippets
codex-agentic index clean
```

Notes
- Custom prompts under your Codex prompts directory are auto‑discovered and appear as additional commands.

Screenshots
-----------
- Put screenshots in `docs/images/` and reference them here. Suggested shots:
  - CLI `/model` picker showing Ollama entries
  - Collapsed “Thinking” chevron (▶) and expanded (▼)
  - ACP in an IDE with `/reasoning summary`

CI
--

GitHub Actions builds on Linux and macOS, runs `fmt`, Clippy (`-D warnings`), and tests. See `.github/workflows/ci.yml`.

Releases
--------

- Tag to release: `git tag -a vX.Y.Z -m "vX.Y.Z" && git push --tags`
- CI builds platform tarballs and publishes a GitHub Release with SHA256SUMS.

Development
-----------

- `cd codex-agentic && cargo build --release`
- `make fmt && make clippy && make test`

License
-------

Apache-2.0. See `LICENSE`.
