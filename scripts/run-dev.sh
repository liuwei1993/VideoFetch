#!/usr/bin/env bash
# Run VideoFetch in development mode (Tauri + Vite).
# Usage: ./scripts/run-dev.sh
#        bash scripts/run-dev.sh
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

if [[ -f "$HOME/.cargo/env" ]]; then
  # shellcheck source=/dev/null
  . "$HOME/.cargo/env"
fi

if ! command -v rustc >/dev/null 2>&1; then
  echo "rustc not found; install Rust (https://rustup.rs/) first" >&2
  exit 1
fi

if ! command -v node >/dev/null 2>&1; then
  echo "node not found; install Node.js 18+ first" >&2
  exit 1
fi

if ! command -v npm >/dev/null 2>&1; then
  echo "npm not found; install Node.js 18+ first" >&2
  exit 1
fi

# Same proxy sniff as other scripts (crates / GitHub often need it).
if [[ -z "${https_proxy:-}${HTTPS_PROXY:-}${http_proxy:-}${HTTP_PROXY:-}" ]] \
  && curl -fsS --max-time 3 -x http://127.0.0.1:57890 https://github.com >/dev/null 2>&1; then
  export https_proxy="${https_proxy:-http://127.0.0.1:57890}"
  export http_proxy="${http_proxy:-http://127.0.0.1:57890}"
  echo "Using local HTTP proxy $https_proxy"
fi

echo "Ensuring Linux sidecars (yt-dlp / ffmpeg / ffprobe)..."
bash "$root/scripts/fetch-linux-sidecars.sh"

if [[ ! -d node_modules ]]; then
  echo "Installing npm dependencies..."
  npm install
fi

echo "Starting VideoFetch (tauri dev)..."
exec npm run tauri -- dev "$@"
