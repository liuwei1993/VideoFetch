# Tauri Video Downloader Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship a Tauri 2 + React desktop app that downloads YouTube/Bilibili via yt-dlp into `~/web-videos/<category>/` with folder-based category management.

**Architecture:** React UI invokes Tauri commands. Rust owns settings JSON, library filesystem ops, and spawns yt-dlp (bundled sidecar preferred; fallback to `yt-dlp` / `uvx`). Progress streams as Tauri events. Categories are first-level directories under the library root.

**Tech Stack:** Tauri 2, React 19, TypeScript, Vite, Rust, yt-dlp, ffmpeg

---

## File map

```
package.json, vite.config.ts, index.html, tsconfig*.json
src/
  main.tsx, App.tsx, App.css
  types.ts
  api.ts                    # invoke wrappers
  views/DownloadView.tsx
  views/LibraryView.tsx
  views/SettingsView.tsx
src-tauri/
  Cargo.toml, tauri.conf.json, capabilities/default.json
  src/lib.rs, main.rs
  src/settings.rs
  src/library.rs
  src/site.rs
  src/download.rs
  binaries/                 # optional yt-dlp sidecar (gitignored if large)
docs/superpowers/plans/2026-09-20-tauri-video-downloader.md
README.md
```

---

### Task 1: Scaffold Tauri 2 + React TS

**Files:**
- Create: project root app files via `create-tauri-app`
- Modify: `.gitignore` (keep downloads ignore; add Tauri targets)

- [ ] **Step 1: Create feature branch**

```bash
cd /home/simon/codes/video-downloader
git checkout -b feat/tauri-client
```

- [ ] **Step 2: Scaffold into repo (force, non-interactive)**

```bash
npm create tauri-app@latest . -- --template react-ts --manager npm --yes --force --tauri-version 2 --identifier com.simon.webvideos
```

- [ ] **Step 3: Install deps and verify build tooling**

```bash
npm install
# Ensure tauri CLI available
npx tauri --version
```

Expected: Tauri 2.x CLI prints version.

- [ ] **Step 4: Commit scaffold**

```bash
git add -A
git commit -m "chore: scaffold Tauri 2 React TypeScript app"
```

---

### Task 2: Settings module (Rust)

**Files:**
- Create: `src-tauri/src/settings.rs`
- Modify: `src-tauri/src/lib.rs`
- Test: unit tests inside `settings.rs`

- [ ] **Step 1: Implement `Settings` with defaults and path expand**

```rust
// settings.rs — key types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub library_root: String,       // default "~/web-videos"
    pub default_quality: String,    // "720" | "1080" | "best"
    pub last_category: String,      // default "未分类"
    pub youtube_proxy: String,      // default "http://127.0.0.1:57890"
    pub bilibili_use_proxy: bool,   // default false
    pub cookie_file: Option<String>,
}

pub fn expand_path(path: &str) -> PathBuf { /* ~ -> home */ }
pub fn settings_path(app: &AppHandle) -> PathBuf { /* app config_dir/settings.json */ }
pub fn load_settings(app: &AppHandle) -> Result<Settings, String> { … }
pub fn save_settings(app: &AppHandle, s: &Settings) -> Result<(), String> { … }
```

- [ ] **Step 2: Unit test expand + default serde**

```rust
#[test]
fn expand_tilde() {
    let p = expand_path("~/web-videos");
    assert!(p.is_absolute());
    assert!(p.ends_with("web-videos"));
}
```

Run: `cd src-tauri && cargo test settings:: -- --nocapture`  
Expected: PASS

- [ ] **Step 3: Register commands `get_settings`, `save_settings`**

- [ ] **Step 4: Commit**

```bash
git commit -m "feat: add settings load/save with defaults"
```

---

### Task 3: Library filesystem (categories + videos)

**Files:**
- Create: `src-tauri/src/library.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Implement ensure/list/create/rename/delete category + list/move/delete/open video**

```rust
pub const UNCATEGORIZED: &str = "未分类";
pub fn ensure_library_root(root: &Path) -> Result<(), String>;
pub fn list_categories(root: &Path) -> Result<Vec<String>, String>;
pub fn create_category(root: &Path, name: &str) -> Result<(), String>;
pub fn rename_category(root: &Path, from: &str, to: &str) -> Result<(), String>;
pub fn delete_category(root: &Path, name: &str, force: bool) -> Result<(), String>;

#[derive(Serialize)]
pub struct VideoItem {
    pub name: String,
    pub path: String,
    pub size: u64,
}

pub fn list_videos(root: &Path, category: &str) -> Result<Vec<VideoItem>, String>;
pub fn move_video(root: &Path, from_cat: &str, to_cat: &str, filename: &str) -> Result<(), String>;
pub fn delete_video(root: &Path, category: &str, filename: &str) -> Result<(), String>;
pub fn open_video(path: &Path) -> Result<(), String>; // opener / xdg-open
```

Video extensions: `mp4`, `webm`, `mkv`, `avi`.

- [ ] **Step 2: Tests with tempdir**

```rust
#[test]
fn create_and_list_category() {
    let dir = tempfile::tempdir().unwrap();
    ensure_library_root(dir.path()).unwrap();
    create_category(dir.path(), "脱口秀").unwrap();
    let cats = list_categories(dir.path()).unwrap();
    assert!(cats.contains(&"未分类".into()));
    assert!(cats.contains(&"脱口秀".into()));
}
```

Add `tempfile` to Cargo.toml `[dev-dependencies]`.

Run: `cargo test library::`  
Expected: PASS

- [ ] **Step 3: Wire Tauri commands + call `ensure_library_root` on setup**

- [ ] **Step 4: Commit**

```bash
git commit -m "feat: folder-based category and video library ops"
```

---

### Task 4: Site detection + download via yt-dlp

**Files:**
- Create: `src-tauri/src/site.rs`, `src-tauri/src/download.rs`
- Modify: `src-tauri/Cargo.toml`, `src-tauri/src/lib.rs`, `src-tauri/capabilities/default.json` (shell/sidecar if needed)

- [ ] **Step 1: Site + format helpers**

```rust
pub enum Site { Youtube, Bilibili, Unknown }
pub fn detect_site(url: &str) -> Site;
pub fn format_selector(quality: &str) -> String {
    match quality {
        "1080" => "bv*[height<=1080]+ba/b[height<=1080]".into(),
        "best" => "bv*+ba/b".into(),
        _ => "bv*[height<=720]+ba/b[height<=720]".into(),
    }
}
pub fn proxy_for(site: Site, settings: &Settings) -> Option<String>;
```

Tests for youtube.com, youtu.be, bilibili.com, b23.tv.

- [ ] **Step 2: Resolve yt-dlp binary**

Order: (1) env `WEB_VIDEOS_YTDLP`, (2) sidecar next to resource, (3) `yt-dlp` on PATH, (4) `uvx --from yt-dlp yt-dlp`.

- [ ] **Step 3: `start_download` spawns process**

Args include: `--newline`, `-f <selector>`, `--merge-output-format mp4`, `-o <category>/<title> [%(id)s].%(ext)s`, optional `--proxy`, `--js-runtimes node` if `node` on PATH, `--no-playlist`.

Emit events:
- `download-progress` `{ percent?: f64, line: string }`
- `download-finished` `{ path: string }`
- `download-error` `{ message: string }`

On success: update `last_category` in settings.

Use `tauri::async_runtime::spawn` + `tokio::process` or `std::process` with piped stdout.

- [ ] **Step 4: Manual smoke (optional in CI)**

```bash
# With proxy up:
# invoke download to ~/web-videos/未分类 for short test URL if available
```

- [ ] **Step 5: Commit**

```bash
git commit -m "feat: yt-dlp download with per-site proxy and progress events"
```

---

### Task 5: Frontend API + three views

**Files:**
- Create: `src/types.ts`, `src/api.ts`, `src/views/*.tsx`
- Modify: `src/App.tsx`, `src/App.css`, `src/main.tsx`

- [ ] **Step 1: Typed invoke wrappers matching Rust commands**

- [ ] **Step 2: App shell with tabs: 下载 / 库 / 设置**

- [ ] **Step 3: DownloadView** — URL, category select (+ create), quality, start, progress, log via `listen`

- [ ] **Step 4: LibraryView** — category sidebar, video list, move/open/delete

- [ ] **Step 5: SettingsView** — bind all Settings fields, save

- [ ] **Step 6: Commit**

```bash
git commit -m "feat: React UI for download, library, and settings"
```

---

### Task 6: Permissions, README, run check

**Files:**
- Modify: `src-tauri/capabilities/default.json`, `src-tauri/tauri.conf.json`
- Create/Modify: `README.md`

- [ ] **Step 1: Allow shell/process, dialog (pick library folder if used), opener**

- [ ] **Step 2: README — deps (ffmpeg, Node for YouTube JS, proxy), `npm run tauri dev`, default paths**

- [ ] **Step 3: `npm run tauri build` or at least `cargo check` + `npm run build`**

Expected: compile succeeds.

- [ ] **Step 4: Commit**

```bash
git commit -m "docs: README and Tauri capabilities for downloader"
```

---

## Spec coverage checklist

| Spec item | Task |
|---|---|
| Tauri 2 + React + TS | 1 |
| `~/web-videos` + folder categories | 3 |
| Remember last category | 2, 4 |
| Default 720p configurable | 2, 4, 5 |
| Per-site proxy | 4 |
| Cookie field persist only | 2, 5 |
| Download / Library / Settings UI | 5 |
| yt-dlp + ffmpeg merge | 4, 6 (README) |
| Progress events | 4, 5 |

## Out of scope (per spec)

In-app player, playlists, nested categories, cookie wiring into yt-dlp.
