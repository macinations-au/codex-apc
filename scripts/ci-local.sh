#!/usr/bin/env bash
set -euo pipefail

echo "[ci-local] Toolchain:" >&2
rustc --version >&2 || true
cargo --version >&2 || true

export CARGO_TERM_COLOR=always
export CARGO_NET_GIT_FETCH_WITH_CLI=${CARGO_NET_GIT_FETCH_WITH_CLI:-true}

run_crate() {
  local dir=$1
  echo "[ci-local] >> $dir" >&2
  (cd "$dir" && cargo fmt --all -- --check)
  (cd "$dir" && cargo clippy -- -D warnings)
  (cd "$dir" && cargo test)
  (cd "$dir" && cargo build --release)
}

run_crate codex-acp
run_crate codex-tui
run_crate codex-agentic

echo "[ci-local] OK" >&2

