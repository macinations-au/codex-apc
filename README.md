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

Using Upstream Commands (The “cli” Bridge)
-----------------------------------------

codex-agentic includes the upstream Codex CLI. Use the `cli --` subcommand to pass commands straight through.

```bash
# Show the upstream help with all commands you’re expecting
codex-agentic cli -- --help

# Examples
codex-agentic cli -- login
codex-agentic cli -- logout
codex-agentic cli -- exec -e 'echo hello'
codex-agentic cli -- resume --last
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

Update & Version
----------------

- The TUI shows an “Update available” message when a newer release is out.
- It links to this repo’s README (you’re here) and can show a one‑liner installer.
- Versioning: we follow `0.39.0-apc.y` (our patch number is `y`).

Notes
- Custom prompts under your Codex prompts directory are auto‑discovered and appear as additional commands.



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
\n<!-- ci: trigger run 2025-09-23T00:51:14Z -->
