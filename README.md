codex-apc
================

[![MSRV 1.89+](https://img.shields.io/badge/MSRV-1.89%2B-blue.svg)](codex-acp/rust-toolchain.toml)
[![Rust Edition 2024](https://img.shields.io/badge/Edition-2024-blueviolet.svg)](https://doc.rust-lang.org/edition-guide/rust-2024/index.html)
<!-- Update the CI badge after pushing to GitHub: replace ORG/REPO -->
![CI](https://img.shields.io/github/actions/workflow/status/macinations-au/codex-apc/ci.yml?label=CI)

Agent Client Protocol (ACP) agent that bridges the OpenAI Codex runtime to ACP‑capable clients such as Zed. This repo contains:

- `codex-acp/` — the Rust ACP agent (primary deliverable)
- `scripts/` — helper scripts for running the agent from a built binary

Install
-------

- One‑liner from GitHub:
  - `bash <(curl -fsSL https://raw.githubusercontent.com/macinations-au/codex-apc/main/scripts/install.sh) --repo macinations-au/codex-apc`
- Or install from source: `cargo install --path codex-acp`
- Or build locally and run the script wrapper: `cd codex-acp && make release && ../scripts/codex-acp.sh`

Quick Start
-----------

- Prereqs: Rust 1.89+ (pinned in `codex-acp/rust-toolchain.toml`).
- Build:
  - `cd codex-acp && make build` (or `make release` for optimized)
- Run a quick stdio smoke test:
  - `cd codex-acp && make smoke`
- Run the agent binary (stdio JSON‑RPC):
  - `./scripts/codex-acp.sh`

Zed Integration
---------------

Add to your Zed settings:

```json
"agent_servers": {
  "Codex": {
    "command": "codex-acp",
    "args": [],
    "env": { "RUST_LOG": "info" }
  }
}
```

Ensure `codex-acp` is on your `PATH` (e.g., `cargo install --path codex-acp`), or point Zed at `scripts/codex-acp.sh`.

Screenshots
-----------
<img width="2418" height="1202" alt="image" src="https://github.com/user-attachments/assets/1bac602e-4a33-49e8-b779-9fe3d86d6e53" />

```
docs/zed-integration.png
```


CI
--

GitHub Actions builds on Linux and macOS, checking format, clippy (`-D warnings`), and tests. See `.github/workflows/ci.yml`.

Development
-----------

Useful commands in `codex-acp/Makefile`:
- `make build`, `make release`
- `make fmt`, `make clippy`, `make check`, `make test`

See `codex-acp/README.md` for details, including `/status` and other slash commands.

Releases
--------

- Tag a release: `git tag -a v0.1.0 -m "v0.1.0" && git push --tags`.
- CI auto-builds release artifacts on tags (Linux x86_64, macOS x86_64 + arm64) and publishes a GitHub Release with tarballs and SHA256SUMS.
- Artifacts contain: `codex-acp` binary, `README.md`, `README-codex-acp.md`, and `LICENSE`.

License
-------

Apache-2.0. See `LICENSE`.
