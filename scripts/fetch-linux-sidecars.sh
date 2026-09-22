#!/usr/bin/env bash
# Download standalone yt-dlp + static ffmpeg/ffprobe for the current Rust host.
# Writes both <name> and <name>-<target-triple> under src-tauri/binaries/ (gitignored).
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
bin_dir="$root/src-tauri/binaries"
mkdir -p "$bin_dir"

if [[ -f "$HOME/.cargo/env" ]]; then
  # shellcheck source=/dev/null
  . "$HOME/.cargo/env"
fi

# Honor an existing proxy. If none is set, try the app's usual local proxy so
# GitHub downloads don't stall on networks that block github.com.
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

case "$triple" in
  x86_64-*)
    yt_url="https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux"
    ff_url="https://github.com/yt-dlp/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-linux64-gpl.tar.xz"
    ;;
  aarch64-*)
    yt_url="https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux_aarch64"
    ff_url="https://github.com/yt-dlp/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-linuxarm64-gpl.tar.xz"
    ;;
  *)
    echo "Unsupported target: $triple (x86_64 / aarch64 glibc only)" >&2
    exit 1
    ;;
esac

need_yt=0
need_ff=0
[[ -x "$bin_dir/yt-dlp" && -x "$bin_dir/yt-dlp-$triple" ]] || need_yt=1
[[ -x "$bin_dir/ffmpeg" && -x "$bin_dir/ffmpeg-$triple" \
  && -x "$bin_dir/ffprobe" && -x "$bin_dir/ffprobe-$triple" ]] || need_ff=1

if [[ "$need_yt" -eq 0 && "$need_ff" -eq 0 ]]; then
  echo "Sidecars already present for $triple"
  ls -lh "$bin_dir"
  exit 0
fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

if [[ "$need_yt" -eq 1 ]]; then
  echo "Downloading yt-dlp for $triple..."
  curl -fL --retry 3 --retry-all-errors -o "$tmp/yt-dlp" "$yt_url"
  chmod +x "$tmp/yt-dlp"
  cp "$tmp/yt-dlp" "$bin_dir/yt-dlp"
  cp "$tmp/yt-dlp" "$bin_dir/yt-dlp-$triple"
fi

if [[ "$need_ff" -eq 1 ]]; then
  echo "Downloading ffmpeg/ffprobe static builds for $triple..."
  curl -fL --retry 3 --retry-all-errors -o "$tmp/ffmpeg.tar.xz" "$ff_url"
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
fi

echo "Sidecars ready:"
ls -lh "$bin_dir"
