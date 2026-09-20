# Audio-only MP3 Implementation Plan

> **For agentic workers:** Implement task-by-task. Steps use checkbox syntax.

**Goal:** Add a download-page checkbox for audio-only MP3 at best quality.

**Architecture:** Pass `audioOnly` through the existing `start_download` command. When set, yt-dlp uses `-x --audio-format mp3 --audio-quality 0` instead of video format/merge. Library lists `mp3`.

**Tech Stack:** Tauri + React + yt-dlp + ffmpeg (system)

---

### Task 1: Backend args + yt-dlp audio path

**Files:**
- Modify: `src-tauri/src/download.rs`
- Modify: `src-tauri/src/library.rs` (add `mp3` to `VIDEO_EXTS`)

- [x] Add `audio_only: bool` to `StartDownloadArgs`
- [x] Thread into `run_download`; branch command args
- [x] Improve exit error when audio extract fails (mention ffmpeg)
- [x] Add `mp3` to library extensions

### Task 2: Frontend checkbox + API

**Files:**
- Modify: `src/api.ts`
- Modify: `src/views/DownloadView.tsx`

- [x] Extend `startDownload(..., audioOnly)`
- [x] Checkbox; disable quality when checked; pass flag on start

### Task 3: Verify

- [x] `cargo check` / frontend typecheck as available
- [ ] Manual: checkbox disables quality; video path unchanged
