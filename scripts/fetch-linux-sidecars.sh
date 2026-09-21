#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
bin_dir="$root/src-tauri/binaries"
mkdir -p "$bin_dir"

if [[ -f "$HOME/.cargo/env" ]]; then
  # shellcheck source=/dev/null
  . "$HOME/.cargo/env"
fi

if [[ -z "${https_proxy:-}${HTTPS_PROXY:-}${http_proxy:-}${HTTP_PROXY:-}" ]] \
  && curl -fsS --max-time 3 -x http://127.0.0.1:57890 https://github.com >/dev/null 2>&1; then
  export https_proxy="${https_proxy:-http://127.0.0.1:57890}"
  export http_proxy="${http_proxy:-http://127.0.0.1:57890}"
  echo "Using local HTTP proxy $https_proxy"
fi

triple="${VIDEOFETCH_HOST_TRIPLE:-}"
if [[ -z "$triple" ]]; then
  if command -v rustc >/dev/null 2>&1; then
    triple="$(rustc -vV | sed -n 's/^host: //p')"
  fi
fi
triple="${triple:-x86_64-unknown-linux-gnu}"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

echo "Downloading yt-dlp for $triple..."
curl -fL --retry 3 --retry-all-errors -o "$tmp/yt-dlp" \
  "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux"
chmod +x "$tmp/yt-dlp"
cp "$tmp/yt-dlp" "$bin_dir/yt-dlp"
cp "$tmp/yt-dlp" "$bin_dir/yt-dlp-$triple"

echo "Downloading ffmpeg/ffprobe static builds..."
curl -fL --retry 3 --retry-all-errors -o "$tmp/ffmpeg.tar.xz" \
  "https://github.com/yt-dlp/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-linux64-gpl.tar.xz"
tar -xJf "$tmp/ffmpeg.tar.xz" -C "$tmp"
ffdir="$(find "$tmp" -type d -name 'bin' | head -1)"
if [[ -z "$ffdir" || ! -x "$ffdir/ffmpeg" || ! -x "$ffdir/ffprobe" ]]; then
  echo "ffmpeg archive did not contain bin/ffmpeg and bin/ffprobe" >&2
  exit 1
fi
cp "$ffdir/ffmpeg" "$bin_dir/ffmpeg"
cp "$ffdir/ffprobe" "$bin_dir/ffprobe"
cp "$ffdir/ffmpeg" "$bin_dir/ffmpeg-$triple"
cp "$ffdir/ffprobe" "$bin_dir/ffprobe-$triple"
chmod +x "$bin_dir/ffmpeg" "$bin_dir/ffprobe" "$bin_dir/ffmpeg-$triple" "$bin_dir/ffprobe-$triple"

echo "Sidecars ready:"
ls -lh "$bin_dir"
