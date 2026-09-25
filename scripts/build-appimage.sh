#!/usr/bin/env bash
# Build a Linux AppImage for VideoFetch (yt-dlp / ffmpeg sidecars included).
# Usage: bash scripts/build-appimage.sh
# Output: .build/*.AppImage
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

out_dir="$root/.build"
mkdir -p "$out_dir"

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

# Same proxy sniff as scripts/fetch-linux-sidecars.sh (GitHub / crates often need it).
if [[ -z "${https_proxy:-}${HTTPS_PROXY:-}${http_proxy:-}${HTTP_PROXY:-}" ]] \
  && curl -fsS --max-time 3 -x http://127.0.0.1:57890 https://github.com >/dev/null 2>&1; then
  export https_proxy="${https_proxy:-http://127.0.0.1:57890}"
  export http_proxy="${http_proxy:-http://127.0.0.1:57890}"
  echo "Using local HTTP proxy $https_proxy"
fi

triple="$(rustc -vV | sed -n 's/^host: //p')"
case "$triple" in
  x86_64-unknown-linux-gnu | aarch64-unknown-linux-gnu) ;;
  *-musl)
    echo "AppImage sidecar builds need glibc (got $triple). Use a glibc host, not Alpine/musl." >&2
    exit 1
    ;;
  *)
    echo "Unsupported host for AppImage: $triple (x86_64 / aarch64 linux-gnu only)" >&2
    exit 1
    ;;
esac

if [[ ! -d node_modules ]]; then
  echo "Installing npm dependencies..."
  npm install
fi

echo "Fetching Linux sidecars (yt-dlp / ffmpeg / ffprobe)..."
bash "$root/scripts/fetch-linux-sidecars.sh"

echo "Building AppImage for $triple..."
npm run tauri -- build --bundles appimage

bundle_dir="$root/src-tauri/target/release/bundle/appimage"
mapfile -t images < <(find "$bundle_dir" -maxdepth 1 -type f -name '*.AppImage' 2>/dev/null | sort)

if [[ ${#images[@]} -eq 0 ]]; then
  echo "Build finished but no AppImage found under $bundle_dir" >&2
  exit 1
fi

# Drop previous copies so .build only keeps the latest build artifacts.
rm -f "$out_dir"/*.AppImage

copied=()
for f in "${images[@]}"; do
  dest="$out_dir/$(basename "$f")"
  cp -f "$f" "$dest"
  chmod +x "$dest"
  copied+=("$dest")
done

echo
echo "AppImage ready in .build/:"
for f in "${copied[@]}"; do
  ls -lh "$f"
done
echo
echo "Run: ${copied[-1]}"
