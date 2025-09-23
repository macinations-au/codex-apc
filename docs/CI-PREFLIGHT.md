## CI Preflight (Do This Before Pushing/Tagging)

Run these steps locally before opening PRs, merging to `main`, or tagging a release. They mirror CI and catch fmt/clippy/test issues early.

### 1) Mirror CI in Docker (Linux/macOS parity)

```bash
# Build the image and run fmt/clippy/tests/release for all crates
bash scripts/ci-docker.sh
```

What it does
- Uses `rust:1.89-bullseye` with required system deps (openssl, git, curl, pkg-config).
- Runs per crate: `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`, `cargo build --release`.
- Sets `CARGO_NET_GIT_FETCH_WITH_CLI=true` to avoid libcurl throttling seen on GH runners.

Common failures and fixes
- rustfmt diffs → run `cargo fmt --all`, re-run Docker CI.
- clippy lints → fix; only `#[allow]` when strictly justified.
- Tests failing after struct changes → update test initializers for new fields (e.g., `pending_about_save: false` in TUI `ChatWidget`).

### 2) Fast local loop (outside Docker)

```bash
for c in codex-acp codex-tui codex-agentic; do (
  cd "$c" && cargo fmt --all && cargo clippy -- -D warnings && cargo test && cargo build --release
); done
```

### 3) Windows sanity (optional)

CI builds on `windows-latest` (debug; tests skipped). For a local exe without CI:

```bash
docker run --rm -v "$PWD":/w -w /w rust:1.89-bullseye bash -lc '
  export PATH=/usr/local/cargo/bin:$PATH
  apt-get update -qq && DEBIAN_FRONTEND=noninteractive apt-get install -y -qq gcc-mingw-w64-x86-64 >/dev/null
  rustup target add x86_64-pc-windows-gnu
  cd codex-agentic && cargo build --release --target x86_64-pc-windows-gnu
'
```

Note (Linux artifact runtime): Release builds currently run on Ubuntu 24.04 (glibc 2.39). To support older distros (e.g., Debian 12), switch the Release matrix to `ubuntu-22.04`.

### 4) Version bump and release tagging

```bash
# Bump versions across crates
sed -i '' -e 's/^version = ".*"/version = "0.39.0-apc.X"/' codex-agentic/Cargo.toml
sed -i '' -e 's/^version = ".*"/version = "0.39.0-apc.X"/' codex-tui/Cargo.toml
sed -i '' -e 's/^version = ".*"/version = "0.39.0-apc.X"/' codex-acp/Cargo.toml

# Re-run Docker CI (must be green)
bash scripts/ci-docker.sh

# Push + tag
git add codex-*/Cargo.toml
git commit -m "release: bump to v0.39.0-apc.X"
git push origin main
git tag -a v0.39.0-apc.X -m "v0.39.0-apc.X"
git push origin v0.39.0-apc.X
```

### Summary
- Always run `scripts/ci-docker.sh` before pushing to main or tagging.
- Keep fmt/clippy/test green; update tests with new fields.
