#!/usr/bin/env bash
set -euo pipefail
export RUST_LOG="${RUST_LOG:-info}"

# Launcher that keeps the installed binary (~/.cargo/bin/codex-agentic) in lockstep
# with the freshest local build so editor integrations pick up the same version.

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
REL="$ROOT_DIR/codex-agentic/target/release/codex-agentic"
DBG="$ROOT_DIR/codex-agentic/target/debug/codex-agentic"
INST="$HOME/.cargo/bin/codex-agentic"

newest() {
  local a="$1" b="$2"
  if [[ -x "$a" && -x "$b" ]]; then
    if [[ "$a" -nt "$b" ]]; then echo "$a"; else echo "$b"; fi
  elif [[ -x "$a" ]]; then echo "$a"; else echo "$b"; fi
}

if [[ "${CODEX_AGENTIC_BIN:-}" == "debug" && -x "$DBG" ]]; then
  CHOSEN="$DBG"
else
  CHOSEN="$(newest "$REL" "$DBG")"
fi

if [[ ! -x "${CHOSEN:-}" ]]; then
  if [[ -x "$INST" ]]; then
    echo "[codex-agentic.sh] Using installed binary: $INST" >&2
    exec "$INST" "$@"
  fi
  echo "[codex-agentic.sh] No codex-agentic binary found. Build with: (cd codex-agentic && cargo build)" >&2
  exit 1
fi

mkdir -p "$(dirname "$INST")"
if [[ ! -x "$INST" || "$CHOSEN" -nt "$INST" ]]; then
  if install -m 0755 "$CHOSEN" "$INST" 2>/dev/null; then
    echo "[codex-agentic.sh] Synced installed binary -> $INST" >&2
  else
    echo "[codex-agentic.sh] Warn: could not update $INST (permissions)" >&2
  fi
fi

echo "[codex-agentic.sh] Running: $CHOSEN $*" >&2
exec "$CHOSEN" "$@"

