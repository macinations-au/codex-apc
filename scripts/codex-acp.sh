#!/usr/bin/env bash
set -euo pipefail
export RUST_LOG="${RUST_LOG:-info}"

# This launcher keeps the installed binary (~/.cargo/bin/codex-acp) in lockstep
# with the freshest repo build so other integrations (e.g. CodexExec) pick up
# the same version automatically.

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
REL="$ROOT_DIR/codex-acp/target/release/codex-acp"
DBG="$ROOT_DIR/codex-acp/target/debug/codex-acp"
INST="$HOME/.cargo/bin/codex-acp"

# Helper: choose newest of two executables
newest() {
  local a="$1" b="$2"
  if [[ -x "$a" && -x "$b" ]]; then
    if [[ "$a" -nt "$b" ]]; then echo "$a"; else echo "$b"; fi
  elif [[ -x "$a" ]]; then echo "$a"; else echo "$b"; fi
}

# Pick freshest repo build (honor CODEX_ACP_BIN=debug override)
if [[ "${CODEX_ACP_BIN:-}" == "debug" && -x "$DBG" ]]; then
  CHOSEN="$DBG"
else
  CHOSEN="$(newest "$REL" "$DBG")"
fi

# If no repo build exists, fall back to installed binary
if [[ ! -x "${CHOSEN:-}" ]]; then
  if [[ -x "$INST" ]]; then
    echo "[codex-acp.sh] Using installed binary: $INST" >&2
    exec "$INST"
  fi
  echo "[codex-acp.sh] No codex-acp binary found. Build with: (cd codex-acp && cargo build)" >&2
  exit 1
fi

# Sync installed binary to freshest repo build (lockstep)
mkdir -p "$(dirname "$INST")"
if [[ ! -x "$INST" || "$CHOSEN" -nt "$INST" ]]; then
  if install -m 0755 "$CHOSEN" "$INST" 2>/dev/null; then
    echo "[codex-acp.sh] Synced installed binary -> $INST" >&2
  else
    echo "[codex-acp.sh] Warn: could not update $INST (permissions)" >&2
  fi
fi

echo "[codex-acp.sh] Running: $CHOSEN" >&2
exec "$CHOSEN"
