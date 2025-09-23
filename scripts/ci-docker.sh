#!/usr/bin/env bash
set -euo pipefail

IMAGE_NAME=${IMAGE_NAME:-codex-local-ci}
DOCKERFILE=${DOCKERFILE:-Dockerfile.ci}

echo "[docker-ci] Building image $IMAGE_NAME from $DOCKERFILE" >&2
docker build -t "$IMAGE_NAME" -f "$DOCKERFILE" .

echo "[docker-ci] Running CI in container" >&2
docker run --rm \
  -e CARGO_NET_GIT_FETCH_WITH_CLI=true \
  -v "$PWD":/w -w /w \
  "$IMAGE_NAME" bash scripts/ci-local.sh

