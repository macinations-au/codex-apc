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

## Useful Commands
```bash
# Build & test locally (single binary)
(cd codex-agentic && cargo fmt --all && cargo clippy -- -D warnings && cargo test && cargo build --release)

# Verify installed vs local build
which -a codex-agentic
ls -lh ~/.cargo/bin/codex-agentic codex-agentic/target/release/codex-agentic
```
