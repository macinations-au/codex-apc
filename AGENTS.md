# AGENTS.md

This file gives agents (and humans) the canonical instructions for working in this repository.

Scope
- Applies to the entire repository. Create a deeper `AGENTS.md` if a subdirectory needs overrides.

## Coding & Workflow
- Keep changes minimal and focused; avoid refactors outside the task.
- Match existing style and structure.
- Prefer root‑cause fixes over band‑aids.
- Ask before making destructive changes.

## Formatting (Markdown)
- Always respond in valid Markdown.
- Use headings (## …), short bullet lists, and fenced code blocks with language tags (```bash, ```json, ```rust).
- Don’t mix code and prose on the same line; put code in fenced blocks.
- Keep answers concise and actionable; avoid preambles and meta commentary.
- For long answers, include a short Summary section at the end.

## Reasoning Output
- Do not stream chain‑of‑thought by default. Provide final answers plus brief bullet rationales when useful.
- The runtime default is “thoughts off”; toggle with `//thoughts on|off` if explicitly requested.

## Release Process
This project publishes binaries via GitHub Actions when a semver tag is pushed (e.g., `v0.1.3`).

- Workflow files: `.github/workflows/release.yml` and `.github/workflows/ci.yml`.
- Targets (Release): `ubuntu-latest`, `macos-14`.
- Artifact: `codex-agentic-vX.Y.Z-<os>-<arch>.tar.gz` and `SHA256SUMS.txt`.

Steps to cut a release
```bash
# 1) Bump crate version
sed -i '' -e 's/^version = ".*"/version = "0.1.4"/' codex-agentic/Cargo.toml

# 2) Build and lint locally (single binary)
(cd codex-agentic && cargo fmt --all && cargo clippy -- -D warnings && cargo build --release)

# 3) Commit + tag + push
git add codex-agentic/Cargo.toml
git commit -m "release(codex-agentic): bump version to v0.1.4"
git push origin main

git tag -a v0.1.4 -m "v0.1.4"
git push origin v0.1.4
```

## CI Preflight (Local Checks)

- Always mirror CI locally before pushing or tagging. See docs/CI-PREFLIGHT.md for step‑by‑step commands to:
  - Run the Dockerized CI mirror (fmt, clippy, tests, release builds) across all crates.
  - Iterate quickly with local cargo commands per crate.
  - Optionally sanity‑check Windows builds.
  - Validate ACP `/status` for `--yolo-with-search`.


## Known Release/CI Failures (and Fixes)
During Sept 2025 we saw “Build (release)” failures on the Release workflow and “Build (debug)” failures on CI for tags `v0.1.1`–`v0.1.3`. Root causes and permanent fixes applied:

- Symptom: Release job failed at step “Build (release)”. CI failed at “Build (debug)”.
- Likely causes on GH runners:
  - `cargo build --locked` acquired a dependency set incompatible with Rust 1.89 or the runner image.
  - Git fetches for `git` dependencies throttled/blocked by the libcurl transport.
  - Deprecated macOS runner (`macos-13`) no longer available.

Fixes already in repo
- Release workflow (`.github/workflows/release.yml`):
  - Removed `macos-13` from matrix; kept `ubuntu-latest`, `macos-14`.
  - Added `fetch-depth: 0` to checkout so tag context is available.
  - Dropped `--locked` for the build; added `CARGO_NET_GIT_FETCH_WITH_CLI: true` and `rustc --version` logging.
- CI workflow (`.github/workflows/ci.yml`):
  - Dropped `--locked` for build/test; added `CARGO_NET_GIT_FETCH_WITH_CLI: true`.

What to do if a Release fails again
1) Open the failing run (Actions → Release). Inspect the “Build (release)” step logs.
2) If it’s a network/fetch error, re‑run the job; the CLI fetch setting usually resolves intermittent issues.
3) If a dependency version error appears, reproduce locally without `--locked`, then commit a version bump or pin as needed.
4) If the runner image fails (e.g., macOS deprecation), update the runner matrix.
5) Re‑tag with the next patch version (e.g., `v0.1.5`) to re‑publish.

Quick re‑tag to retrigger after workflow edits
```bash
git tag -d v0.1.4
git tag -a v0.1.4 -m "v0.1.4"
git push -f origin v0.1.4
```

## Installer & Lockstep Launcher
- Installer: `scripts/install.sh` downloads the latest (or specified) release and installs the `codex-agentic` binary to `~/.cargo/bin`.
- Launcher: `scripts/codex-agentic.sh` runs the freshest repo build and syncs `~/.cargo/bin/codex-agentic` to match, keeping editor integrations in lockstep.

## Upgrade Banner & Version Detection
codex-tui performs a lightweight update check on startup and can display an upgrade banner inside the TUI. We tailor this behavior to codex-agentic so users are directed to this repo (README) rather than upstream.

- Detection source (default): GitHub Releases “latest” for this repo.
- Display target (default): this repo’s README page.
- Install command (optional): surfaced if provided by CI/packaging.

Environment variables (read at runtime by `codex-agentic` before launching the TUI):

- `CODEX_AGENTIC_UPDATE_REPO` — `owner/repo` slug (e.g., `macinations-au/codex-apc`). If set, detection points to `https://api.github.com/repos/<slug>/releases/latest`.
- `CODEX_AGENTIC_UPGRADE_CMD` — shell one‑liner for upgrades; shown as “Run <cmd> to update.”
- `CODEX_UPGRADE_URL` — URL to show when no command is supplied; defaults to `https://github.com/<slug>` (README).
- `CODEX_UPDATE_LATEST_URL` — fully‑qualified override for the releases API endpoint (rarely needed when `CODEX_AGENTIC_UPDATE_REPO` is set).
- `CODEX_CURRENT_VERSION` — version string used for comparison (defaults to `codex-agentic`’s Cargo version).
- `CODEX_DISABLE_UPDATE_CHECK=1` — disables the banner entirely.

CI/Release defaults:
- Both `.github/workflows/ci.yml` and `.github/workflows/release.yml` set:
  - `CODEX_AGENTIC_UPDATE_REPO=${{ github.repository }}`
  - `CODEX_AGENTIC_UPGRADE_CMD=bash <(curl -fsSL https://raw.githubusercontent.com/${{ github.repository }}/main/scripts/install.sh)`
  - `CODEX_UPGRADE_URL=https://github.com/${{ github.repository }}`

Version comparison:
- Accepts `vX.Y.Z`, `rust-vX.Y.Z`, or `X.Y.Z[-apc.N]`.
- Pre-releases like `beta`/`rc` are ignored for “newer?” checks; `-apc.N` compares numerically when `X.Y.Z` matches.

## Useful Commands
```bash
# Build & test locally (single binary)
(cd codex-agentic && cargo fmt --all && cargo clippy -- -D warnings && cargo test && cargo build --release)

# Verify installed vs local build
which -a codex-agentic
ls -lh ~/.cargo/bin/codex-agentic codex-agentic/target/release/codex-agentic
```

## Codebase Review & Memorization

- Review artifact: the latest codebase report is saved at `.codex/review-codebase.json` with the Markdown body in `report.markdown`.
- TUI behavior:
  - On session start, if a saved report exists, the TUI submits a background turn asking the model to memorize it and to reply exactly `Agent memorised.`
  - This does not block input. The acknowledgement is shown as a small status line later. It runs once per session.
- ACP behavior:
  - The first `/about-codebase` quick view shows the saved report and then synchronously asks the model to memorize it (once per session). This avoids competing event readers.
- Why: keeps the model aligned with the repo context without stalling the UI or starving other turns.
- Markdown fixes: headings are sanitized to start on a new line; streaming also inserts a blank line before `#` if needed.
- Avoid background readers in ACP: only one consumer should call `conversation.next_event()` at a time. Do not add background tasks that read conversation events.


## Local Install/Sync (for quick testing)

To install the freshest local build into `~/.cargo/bin` and keep it in lockstep while developing:

```bash
# Build a release binary and sync to ~/.cargo/bin/codex-agentic
(cd codex-agentic && cargo build --release)
scripts/codex-agentic.sh --help >/dev/null 2>&1 || true  # syncs installed binary

# Sanity checks
which codex-agentic
codex-agentic --version
codex-agentic index status
```

Notes
- The launcher script `scripts/codex-agentic.sh` always prefers the newest local binary (release/debug) and syncs it to `~/.cargo/bin/codex-agentic` for editor integrations.
- Add `[skip ci]` to local commit messages when iterating to avoid triggering CI or Release workflows on pushes.
