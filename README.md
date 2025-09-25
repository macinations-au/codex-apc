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
- `scripts/` — helper scripts (`install.sh`, `codex-agentic.sh`)

Install
-------

- One‑liner (installs latest codex-agentic):
  - `bash <(curl -fsSL https://raw.githubusercontent.com/macinations-au/codex-apc/main/scripts/install.sh)`
- From source (local build):
  - `cargo install --path codex-agentic --force`

Quick Start (Simple)
--------------------

- Install (one line):

```bash
bash <(curl -fsSL https://raw.githubusercontent.com/macinations-au/codex-apc/main/scripts/install.sh)
```

- Start chatting (default mode):

```bash
codex-agentic
```

- Use it inside an editor (ACP mode):

```bash
codex-agentic acp
```

That’s it. Think of “cli” as chat in your terminal and “acp” as the version your editor talks to.

Two Ways To Use It (Like You’re 10)
-----------------------------------

- Talk in the Terminal (CLI)
  - Run: `codex-agentic`
  - Type your question. Use `/model` to pick a brain (OpenAI or local Ollama).
  - Want to hide the “thinking”? Press `r` or start with `--reasoning summary`.

- Talk in Your Editor (ACP)
  - Run: `codex-agentic acp` (keep it running)
  - Tell your editor to connect to it (Zed example below).

Bonus: `codex-agentic` can also run all the normal “Codex CLI” commands. See “Using Upstream Commands” below.

Everyday Examples
-----------------

- Pick a model and ask something

```bash
codex-agentic
# then type in the UI:
/model   # choose a model
What is 17 * 23?
```

- Use a local model from Ollama

```bash
codex-agentic
# then type:
/model   # pick something like qwq:latest
```

- Make “thinking” shorter

```bash
codex-agentic --reasoning summary
```

- Run in ACP mode for your editor

```bash
codex-agentic acp --reasoning summary
```

Zed Integration
---------------

Example (two entries: repo launcher and the installed binary):

```json
"agent_servers": {
  "CodexAgentic": {
    "command": "codex-agentic",
    "env": { "RUST_LOG": "info" },
    "args": [
      "--acp",
      "--search"
    ]
  }
}
```

Using Upstream Commands (The “cli” Bridge) + Local Search
--------------------------------------------------------

Local Code Search Index
-----------------------

- Engine
  - Embeds with FastEmbed (BGE small/large) and builds an HNSW ANN graph.
  - Files: `.codex/index/vectors.hnsw` (flat store), `.codex/index/vectors.hnsw.graph`, `.codex/index/vectors.hnsw.data`, `.codex/index/meta.jsonl`, `.codex/index/manifest.json`.
- Build
  - `codex-agentic index build --model bge-small` (or `bge-large`).
- Query
  - `codex-agentic index query "<text>" -k 8 --show-snippets` (TUI `/search` uses the same engine).
- Confidence gating (CLI)
  - Hides results when the top score < 0.60 and prints: `No information exists that matches the request.`
  - Override with `CODEX_INDEX_RETRIEVAL_THRESHOLD=0.70`.

codex-agentic includes the upstream Codex CLI. Use the `cli --` subcommand to pass commands straight through.

```bash
# Show the upstream help with all commands you’re expecting
codex-agentic cli -- --help

# Examples
codex-agentic cli -- login
codex-agentic cli -- logout
codex-agentic cli -- exec -e 'echo hello'
codex-agentic resume --last
codex-agentic resume --yolo --search
# Local semantic code search (same as TUI /search)
codex-agentic search-code "resume picker" -k 8 --show-snippets

# More examples
codex-agentic search-code "<text>" -k 8 --show-snippets --line-number-width 6
codex-agentic search-code "<text>" -k 8 --show-snippets --output json
codex-agentic search-code "<text>" -k 8 --show-snippets --output xml --diff --no-line-numbers
# Same flags work with the index subcommand
codex-agentic index query "<text>" -k 8 --show-snippets --output json --line-number-width 4

codex-agentic cli -- resume --last   # still supported via upstream bridge
codex-agentic cli -- apply --help
```

Why two layers? Because this project adds ACP mode and extra tweaks, but we keep all the familiar Codex CLI features available under `cli --`.

ACP Configuration Recipes (-c/--config)
---------------------------------------

You can set any config key with `-c key=value`. Values are parsed as JSON when possible, otherwise treated as strings. Repeat `-c` to set multiple keys.

- Switch to local provider (Ollama) and pick a model

```bash
codex-agentic acp -c model_provider=\"oss\" -c model=\"qwq:latest\"
```

- Safer auto-exec in your workspace

```bash
codex-agentic acp -c ask_for_approval=\"on-failure\" -c sandbox_mode=\"workspace-write\"
```

- Make “thinking” concise and effort medium

```bash
codex-agentic acp -c model_reasoning_summary=\"concise\" -c model_reasoning_effort=\"medium\"
```

- Hide reasoning completely

```bash
codex-agentic acp -c model_reasoning_summary=\"none\" -c hide_agent_reasoning=true
```

- Set the working directory

```bash
codex-agentic acp -c cwd=\"/path/to/project\"
```

Hints
- Strings need quotes if they contain special characters: `-c model=\"gpt-4o-mini\"`
- Lists use JSON: `-c sandbox_permissions='["disk-full-read-access"]'`
- Prefer first‑class flags when available: `--model`, `--oss`, `--profile`, `--cwd`, `--model-reasoning-effort`, etc.

YOLO Mode With Search (Dangerous)
---------------------------------

One switch that makes the model run without asking, without a sandbox, and enables web search.

```bash
codex-agentic acp --yolo-with-search
```

What it sets under the hood
- `ask_for_approval = "never"`
- `sandbox_mode = "danger-full-access"`
- `tools.web_search_request = true`

Only use this in a throwaway or well‑isolated environment.




Slash commands (high‑use)
-------------------------
- `/model` — open the model picker; includes local Ollama models; saves model only (never provider)
- `/status` — workspace/account/model/tokens
- `/approvals <policy>` — `untrusted | on-request | on-failure | never`
- `/reasoning <hidden|summary|raw>` — collapse or show “thinking”
- `/init` — scaffold an AGENTS.md in the workspace
- `/about-codebase [--refresh|-r]` — show the latest codebase report; if stale (>24h) or changes are detected, it asks you to refresh. Pass `--refresh` to rebuild immediately.
- `/index <status|build|verify|clean …>` — manage the local code index. Examples: `/index status`, `/index build --model bge-small`, `/index clean`.
- `/search <query> [-k N]` — semantic search in your codebase (local). Example: `/search how to start acp server -k 8`.

Codebase Indexing & Retrieval (Local)
-------------------------------------

Local, private code search powers semantic retrieval in chat. Index lives under `.codex/index` and is built fully on your machine (no network calls).

- What it is
  - Engine: FastEmbed (CPU, ONNX). Default model: `bge-small-en-v1.5` (384‑D). Optional: `bge-large-en-v1.5` (1024‑D).
  - On‑disk layout: `.codex/index/{manifest.json, vectors.hnsw, meta.jsonl, analytics.json}`.
  - Analytics: `analytics.json` tracks `{ queries, hits, misses, last_query_ts, last_attempt_ts }`.

- CLI commands

```bash
# Build or refresh (incremental by default)
codex-agentic index build [--model bge-small|bge-large] [--force] [--chunk auto|lines] [--lines 160] [--overlap 32]

# Query top‑K matches (prints ranked hits; add --show-snippets for previews)
codex-agentic index query "<text>" -k 8 --show-snippets

# Status / Verify / Clean
codex-agentic index status
codex-agentic index verify
codex-agentic index clean
```

- TUI & ACP behavior
  - Retrieval injection: before sending your prompt to the model, the agent queries the local index and may inject a short context block titled “Context (top matches from local code index) …”.
  - Confidence gating: injection only happens when the top match score ≥ threshold. Default `CODEX_INDEX_RETRIEVAL_THRESHOLD=0.725`.
  - UI surfacing:
    - TUI shows a compact footer summary like `> 76% -- 3 items found` (not part of the transcript).
    - ACP injects context silently when over threshold (no extra transcript lines).
  - Slash commands: `/index …` mirrors the CLI; `/search …` is a shortcut for `index query`.

- Build/refresh lifecycle
  - First‑run: best‑effort background build when `.codex/index/manifest.json` is missing (respecting disables; output kept quiet).
  - Post‑turn refresh: after each assistant response, a best‑effort incremental refresh may run if the last attempt was more than `CODEX_INDEX_REFRESH_MIN_SECS` ago (default 300s).
  - Periodic maintenance: a lightweight 5‑minute check detects git deltas and triggers an incremental rebuild when files changed.
  - TUI footer: shows “Indexed <relative> • Checked <relative>” based on `manifest.json` and `analytics.json`.

- Environment toggles
  - `CODEX_INDEXING=0` — disable background builds/refresh completely.
  - `CODEX_INDEX_RETRIEVAL=0` — disable retrieval injection in chat.
  - `CODEX_INDEX_RETRIEVAL_THRESHOLD=<float>` — adjust confidence gate (default `0.725`).
  - `CODEX_INDEX_REFRESH_MIN_SECS=<u64>` — min seconds between post‑turn refresh attempts (default `300`).

Ignore Patterns (.index-ignore)
-------------------------------

The indexer respects a repo‑local ignore file at `.index-ignore` (created automatically on first run with sensible defaults). The format is one glob‑like pattern per line; `*` and `?` are supported and lines starting with `#` are comments.

Default entries include:

```
.*
.git
.codex
.idea
.vscode
node_modules
target
dist
build
```

Manage patterns via CLI:

```bash
# Show current patterns and the file path
codex-agentic index ignore --list

# Add/remove patterns (repeat flags to manage several entries)
codex-agentic index ignore --add "*.min.js" --add ".cache/*"
codex-agentic index ignore --remove "*.min.js"

# Reset to defaults
codex-agentic index ignore --reset --list
```


Notes
- MVP focuses on correctness and UX. Index persistence uses flat vectors + JSONL with atomic writes and a persisted HNSW graph for fast ANN queries.
- Chunking defaults to `auto` with a Rust tree‑sitter path; falls back to blank‑line blocks. A `lines` mode is available with `--chunk lines`.

Update & Version
----------------

- The TUI shows an “Update available” message when a newer release is out.
- It links to this repo’s README (you’re here) and can show a one‑liner installer.
- Versioning: we follow `0.39.0-apc.y` (our patch number is `y`).

Advanced: Upgrade Banner & Update Check
---------------------------------------

The TUI performs a lightweight update check and can show an upgrade banner. These env vars control behavior:

- `CODEX_AGENTIC_UPDATE_REPO` — `owner/repo` slug used to build the GitHub Releases API endpoint.
- `CODEX_AGENTIC_UPGRADE_CMD` — one‑liner shown as “Run <cmd> to update.”
- `CODEX_UPGRADE_URL` — fallback URL to open (defaults to this repo’s GitHub page).
- `CODEX_UPDATE_LATEST_URL` — fully‑qualified override for the latest release API.
- `CODEX_CURRENT_VERSION` — overrides version used for comparisons.
- `CODEX_DISABLE_UPDATE_CHECK=1` — disables the banner entirely.

Functional Changes (Sept 2025)
------------------------------
- /about-codebase formatting: fixed run‑on Markdown (headings now start on a new line) in both TUI and ACP.
- Memorize the saved report once per session:
  - TUI: done in the background at startup (non‑blocking); a short “Agent memorised.” status appears later.
  - ACP: done on the first quick view of the report; synchronous so the acknowledgement arrives promptly.
- ACP stability: background readers that competed for conversation events were removed; custom prompt auto‑discovery is disabled in ACP.

What’s New: Local Indexing & Retrieval
--------------------------------------
- New CLI: `codex-agentic index {build,query,status,verify,clean}`.
- New slash commands: `/index …` and `/search …` in both TUI and ACP.
- Retrieval: optional automatic context injection from the local index, gated by `CODEX_INDEX_RETRIEVAL_THRESHOLD` (defaults 0.60–0.65 depending on client). Disable with `CODEX_INDEX_RETRIEVAL=0`.
- Token budget: cap injected context with `CODEX_INDEX_CONTEXT_TOKENS` (approximate tokens; default 800).
- UX: TUI footer displays “Indexed … • Checked …” and a compact confidence summary while composing.

Notes
- Custom prompt auto‑discovery remains in TUI. It is disabled in ACP to avoid event contention.



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

Local Install/Sync (handy while developing)
-------------------------------------------

Keep your installed binary in lockstep with the freshest local build:

```bash
(cd codex-agentic && cargo build --release)
scripts/codex-agentic.sh --help >/dev/null 2>&1 || true  # syncs ~/.cargo/bin/codex-agentic

# Sanity checks
which codex-agentic
codex-agentic --version
codex-agentic index status
```

License
-------

Apache-2.0. See `LICENSE`.
\n<!-- ci: trigger run 2025-09-23T00:51:14Z -->