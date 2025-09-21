#!/usr/bin/env bash
set -euo pipefail
export RUST_LOG="${RUST_LOG:-info}"

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
REL="$ROOT_DIR/codex-acp/target/release/codex-acp"
DBG="$ROOT_DIR/codex-acp/target/debug/codex-acp"

if [[ "${CODEX_ACP_BIN:-}" == "debug" && -x "$DBG" ]]; then
  echo "[codex-acp.sh] Using debug binary" >&2
  exec "$DBG"
fi

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
  echo "[codex-acp.sh] No codex-acp binary found. Build with: (cd codex-acp && cargo build)" >&2
  exit 1
fi

