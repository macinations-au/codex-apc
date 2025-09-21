#!/usr/bin/env bash
set -euo pipefail
# Default logging can be overridden by env
export RUST_LOG="${RUST_LOG:-info}"
# Resolve repo-root relative binary
ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
REL="$ROOT_DIR/codex/codex-rs/target/release/codex-acp"
DBG="$ROOT_DIR/codex/codex-rs/target/debug/codex-acp"

# If CODEX_ACP_BIN=debug is set, force debug (handy while iterating)
if [[ "${CODEX_ACP_BIN:-}" == "debug" && -x "$DBG" ]]; then
  echo "[codex-acp.sh] Using debug binary" >&2
  exec "$DBG"
fi

# Prefer the newer binary by mtime when both exist
if [[ -x "$REL" && -x "$DBG" ]]; then
  if [[ "$DBG" -nt "$REL" ]]; then
    echo "[codex-acp.sh] Debug is newer; using debug binary" >&2
    exec "$DBG"
  else
    echo "[codex-acp.sh] Using release binary" >&2
    exec "$REL"
  fi
elif [[ -x "$REL" ]]; then
  echo "[codex-acp.sh] Using release binary" >&2
  exec "$REL"
elif [[ -x "$DBG" ]]; then
  echo "[codex-acp.sh] Release not found; using debug binary" >&2
  exec "$DBG"
else
  echo "[codex-acp.sh] No codex-acp binary found. Build with: cd codex/codex-rs && cargo build -p acp-server" >&2
  exit 1
fi
