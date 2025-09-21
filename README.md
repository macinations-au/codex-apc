codex-apc
================

[![MSRV 1.89+](https://img.shields.io/badge/MSRV-1.89%2B-blue.svg)](codex-acp/rust-toolchain.toml)
[![Rust Edition 2024](https://img.shields.io/badge/Edition-2024-blueviolet.svg)](https://doc.rust-lang.org/edition-guide/rust-2024/index.html)
![CI](https://img.shields.io/github/actions/workflow/status/macinations-au/codex-apc/ci.yml?label=CI)

An Agent Client Protocol (ACP) agent that bridges the OpenAI Codex runtime to ACP‑capable clients (e.g., Zed). On session start it prints a Markdown banner with the application version and status. A minimal set of custom slash commands is built in, and custom prompts are discovered and exposed as commands automatically.

<img width="2414" height="1354" alt="Untitled" src="https://github.com/user-attachments/assets/40199fd6-eebd-41b0-a73b-eb6bbfa6d406" />



Contents
- `codex-acp/` — Rust ACP agent (primary deliverable)
- `scripts/` — helper scripts (`install.sh`, `codex-acp.sh`)

Install
-------

- One‑liner (defaults to this repo; installs the latest GitHub Release):
  - `bash <(curl -fsSL https://raw.githubusercontent.com/macinations-au/codex-apc/main/scripts/install.sh)`
- From source (local build):
  - `cargo install --path codex-acp --force`

Run
---

- Recommended launcher (keeps installed binary in lockstep with the freshest local build):
  - `./scripts/codex-acp.sh`
  - The script syncs `~/.cargo/bin/codex-acp` to the newest repo build on each run.

Zed Integration
---------------

Example (two entries: repo launcher and the installed binary):

```json
"agent_servers": {
  "Codex": {
    "command": "/Users/<you>/workspace/codex-apc/scripts/codex-acp.sh",
    "env": { "RUST_LOG": "info" }
  },
  "CodexExec": {
    "command": "/Users/<you>/.cargo/bin/codex-acp",
    "env": { "RUST_LOG": "info" }
  }
}
```

Slash Commands (built‑ins)
--------------------------

- `/status` — Markdown status (workspace, account, model, tokens)
- `/init` — create an AGENTS.md template in the workspace
- `/model <slug>` — set session model
- `/approvals <policy>` — `untrusted | on-request | on-failure | never`
- `/thoughts on|off` — show/hide reasoning stream (default: off)

Notes
- Custom prompts under your Codex prompts directory are auto‑discovered and exposed as additional commands.

CI
--

GitHub Actions builds on Linux and macOS, runs `fmt`, Clippy (`-D warnings`), and tests. See `.github/workflows/ci.yml`.

Releases
--------

- Tag to release: `git tag -a vX.Y.Z -m "vX.Y.Z" && git push --tags`
- CI builds platform tarballs and publishes a GitHub Release with SHA256SUMS.

Development
-----------

- `cd codex-acp && make build | make release`
- `make fmt && make clippy && make test`

License
-------

Apache-2.0. See `LICENSE`.
