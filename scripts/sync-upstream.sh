#!/usr/bin/env bash
set -euo pipefail

# Sync git dependencies (codex-core, codex-cli) to a specific upstream commit.
# Usage: scripts/sync-upstream.sh <sha-or-ref>

if [[ $# -lt 1 ]]; then
  echo "Usage: $0 <sha-or-ref>" >&2
  exit 2
fi
SHA="$1"

echo "Syncing openai/codex to $SHA" >&2
(cd codex-acp && cargo update -p codex-core --precise "$SHA" || true)
(cd codex-acp && cargo update -p codex-cli  --precise "$SHA" || true)

echo "Building..." >&2
(cd codex-acp && cargo build)
(cd codex-agentic && cargo build)
echo "Testing..." >&2
(cd codex-acp && cargo test --all || true)
(cd codex-agentic && cargo test --all)
echo "Done." >&2
