#!/usr/bin/env bash
set -euo pipefail

# Simple installer for codex-agentic from GitHub Releases.
#
# Usage examples:
#   REPO=ORG/REPO bash scripts/install.sh
#   bash <(curl -fsSL https://raw.githubusercontent.com/ORG/REPO/main/scripts/install.sh) --repo ORG/REPO
#   bash scripts/install.sh --repo ORG/REPO --version v0.1.0 --bin-dir /usr/local/bin

REPO="${REPO:-macinations-au/codex-apc}"
VERSION="${VERSION:-latest}"
BIN_DIR="${BIN_DIR:-}"
DRY_RUN=0
BIN_NAME="${BIN_NAME:-codex-agentic}"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --repo)
      REPO="$2"; shift 2 ;;
    --version)
      VERSION="$2"; shift 2 ;;
    --bin-dir)
      BIN_DIR="$2"; shift 2 ;;
    --dry-run)
      DRY_RUN=1; shift ;;
    -h|--help)
      cat <<EOF
Usage: $0 --repo ORG/REPO [--version vX.Y.Z] [--bin-dir DIR] [--dry-run]

Environment variables:
  REPO, VERSION, BIN_DIR, BIN_NAME mirror the flags above.
EOF
      exit 0 ;;
    *) echo "Unknown arg: $1" >&2; exit 2 ;;
  esac
done

# REPO can be overridden via --repo or REPO env; defaults to this repo.

uname_s=$(uname -s | tr '[:upper:]' '[:lower:]')
uname_m=$(uname -m)
case "$uname_s" in
  darwin) os="macos" ;;
  linux)  os="linux" ;;
  *) echo "Unsupported OS: $uname_s" >&2; exit 1 ;;
esac
case "$uname_m" in
  x86_64|amd64) arch="x86_64" ;;
  arm64|aarch64) arch="arm64" ;;
  *) echo "Unsupported arch: $uname_m" >&2; exit 1 ;;
esac

if [[ -z "$BIN_DIR" ]]; then
  if [[ -d "$HOME/.local/bin" ]]; then
    BIN_DIR="$HOME/.local/bin"
  else
    BIN_DIR="/usr/local/bin"
  fi
fi

mkdir -p "$BIN_DIR"

if ! command -v curl >/dev/null 2>&1; then
  echo "error: curl is required" >&2
  exit 1
fi

api_url="https://api.github.com/repos/${REPO}"
if [[ "$VERSION" == "latest" ]]; then
  api_url+="/releases/latest"
else
  api_url+="/releases/tags/${VERSION}"
fi

echo "Fetching release metadata: $api_url" >&2
json=$(curl -fsSL "$api_url")

# Determine tag and asset URL
if [[ "$VERSION" == "latest" ]]; then
  tag=$(printf '%s' "$json" | awk -F '"' '/"tag_name"/ {print $4; exit}')
else
  tag="$VERSION"
fi

name_regex="${BIN_NAME}-${tag}-${os}-${arch}\\.tar\\.gz"
asset_url=$(printf '%s' "$json" | awk -v re="$name_regex" -F '"' '$2=="browser_download_url" && $4 ~ re {print $4; exit}')

if [[ -z "$asset_url" ]]; then
  echo "error: could not find asset matching ${name_regex}" >&2
  echo "available assets:" >&2
  printf '%s' "$json" | awk -F '"' '$2=="browser_download_url" {print "  " $4}' >&2
  exit 1
fi

echo "Downloading: $asset_url" >&2
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

curl -fsSL "$asset_url" -o "$work/pkg.tgz"
tar -C "$work" -xzf "$work/pkg.tgz"

if [[ ! -f "$work/$BIN_NAME" ]]; then
  echo "error: extracted tarball missing $BIN_NAME binary" >&2
  exit 1
fi

dest="$BIN_DIR/$BIN_NAME"
echo "Installing to: $dest" >&2

if [[ $DRY_RUN -eq 1 ]]; then
  echo "[dry-run] would install $work/$BIN_NAME -> $dest" >&2
  exit 0
fi

if [[ -w "$BIN_DIR" ]]; then
  install -m 0755 "$work/$BIN_NAME" "$dest"
else
  echo "Elevating privileges to write $BIN_DIR" >&2
  sudo install -m 0755 "$work/$BIN_NAME" "$dest"
fi

echo "Installed $("$dest" --version 2>/dev/null || echo $BIN_NAME)." >&2
echo "Done." >&2
