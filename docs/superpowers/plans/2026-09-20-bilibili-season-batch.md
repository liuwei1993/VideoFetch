# Bilibili Season Batch Download Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Paste a Bilibili season/collection URL, expand it with yt-dlp, download items with configurable concurrency (default 5), and show a Xunlei-like per-item status table.

**Architecture:** Detect collection URLs in `site.rs`; expand via `--flat-playlist` in a new `playlist.rs`; keep single-video path unchanged (`--no-playlist`). Batch mode runs a worker pool (N = `max_concurrent_downloads`) calling existing `run_download` per BV URL, emitting batch/item events. Frontend `DownloadView` builds a task table from those events. Multi-PID tracking so Stop kills all active yt-dlp processes.

**Tech Stack:** Tauri 2 + Rust, React + Ant Design, yt-dlp

**Spec:** `docs/superpowers/specs/2026-09-20-bilibili-season-batch-design.md`

---

### File map

| File | Responsibility |
|------|----------------|
| `src-tauri/src/settings.rs` | `max_concurrent_downloads` (default 5, clamp 1–10) |
| `src-tauri/src/site.rs` | `is_bilibili_collection_url` |
| `src-tauri/src/playlist.rs` | Parse flat-playlist lines; build video URLs; run expand command |
| `src-tauri/src/download.rs` | Multi-PID stop; batch events; worker pool; branch in `start_download` |
| `src-tauri/src/lib.rs` | `mod playlist`; clamp on `save_settings` |
| `src/types.ts` | Settings field + batch event types |
| `src/views/SettingsView.tsx` | 「同时下载数」输入 |
| `src/views/DownloadView.tsx` | Task table + batch listeners + summary |
| `README.md` | Mention collection URL + concurrency setting |

---

### Task 1: Settings — `max_concurrent_downloads`

**Files:**
- Modify: `src-tauri/src/settings.rs`
- Modify: `src/types.ts`
- Modify: `src-tauri/src/lib.rs` (clamp on save)

- [ ] **Step 1: Add failing test** in `settings.rs` tests

```rust
#[test]
fn default_max_concurrent_is_five() {
    let s = Settings::default();
    assert_eq!(s.max_concurrent_downloads, 5);
}

#[test]
fn clamp_max_concurrent() {
    assert_eq!(clamp_max_concurrent(0), 1);
    assert_eq!(clamp_max_concurrent(5), 5);
    assert_eq!(clamp_max_concurrent(99), 10);
}

#[test]
fn missing_max_concurrent_deserializes_to_default() {
    let raw = r#"{
      "library_root": "~/videofetch",
      "default_quality": "720",
      "last_category": "未分类",
      "youtube_proxy": "http://127.0.0.1:57890",
      "bilibili_use_proxy": false,
      "cookie_file": null
    }"#;
    let s: Settings = serde_json::from_str(raw).unwrap();
    assert_eq!(s.max_concurrent_downloads, 5);
}
```

- [ ] **Step 2: Run tests — expect FAIL**

```bash
cd /home/simon/codes/video-downloader/src-tauri && cargo test default_max_concurrent_is_five clamp_max_concurrent missing_max_concurrent -- --nocapture
```

Expected: compile/fail — field or `clamp_max_concurrent` missing.

- [ ] **Step 3: Implement**

In `Settings`:

```rust
#[serde(default = "default_max_concurrent")]
pub max_concurrent_downloads: u32,
```

```rust
fn default_max_concurrent() -> u32 {
    5
}

pub fn clamp_max_concurrent(n: u32) -> u32 {
    n.clamp(1, 10)
}
```

In `Default`: `max_concurrent_downloads: 5`.

In `lib.rs` `save_settings`:

```rust
fn save_settings(app: tauri::AppHandle, mut settings: Settings) -> Result<(), String> {
    settings.max_concurrent_downloads =
        settings::clamp_max_concurrent(settings.max_concurrent_downloads);
    let root = settings::library_root_path(&settings);
    library::ensure_library_root(&root)?;
    settings::save_settings(&app, &settings)
}
```

In `src/types.ts` add: `max_concurrent_downloads: number;`

- [ ] **Step 4: Run tests — expect PASS**

```bash
cd /home/simon/codes/video-downloader/src-tauri && cargo test default_max_concurrent_is_five clamp_max_concurrent missing_max_concurrent -- --nocapture
```

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/settings.rs src-tauri/src/lib.rs src/types.ts
git commit -m "feat: add max_concurrent_downloads setting (default 5)"
```

---

### Task 2: Detect Bilibili collection URLs

**Files:**
- Modify: `src-tauri/src/site.rs`

- [ ] **Step 1: Add failing tests**

```rust
#[test]
fn detect_bilibili_collection_urls() {
    assert!(is_bilibili_collection_url(
        "https://space.bilibili.com/7504289/lists/6254946?type=season"
    ));
    assert!(is_bilibili_collection_url(
        "https://space.bilibili.com/7504289/lists/6254946?type=series"
    ));
    assert!(is_bilibili_collection_url(
        "https://SPACE.BILIBILI.COM/1/lists/2"
    ));
    assert!(!is_bilibili_collection_url(
        "https://www.bilibili.com/video/BV1xx411c7mD"
    ));
    assert!(!is_bilibili_collection_url("https://b23.tv/abcdef"));
    assert!(!is_bilibili_collection_url(
        "https://www.youtube.com/playlist?list=PLxxx"
    ));
}
```

- [ ] **Step 2: Run — expect FAIL**

```bash
cd /home/simon/codes/video-downloader/src-tauri && cargo test detect_bilibili_collection_urls -- --nocapture
```

- [ ] **Step 3: Implement**

```rust
/// Bilibili space season/series collection pages (not single BV pages).
pub fn is_bilibili_collection_url(url: &str) -> bool {
    let lower = url.to_lowercase();
    if !(lower.contains("bilibili.com") || lower.contains("b23.tv")) {
        return false;
    }
    // space.bilibili.com/<mid>/lists/<id>
    lower.contains("space.bilibili.com") && lower.contains("/lists/")
}
```

- [ ] **Step 4: Run — expect PASS**

```bash
cd /home/simon/codes/video-downloader/src-tauri && cargo test detect_bilibili_collection_urls -- --nocapture
```

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/site.rs
git commit -m "feat: detect Bilibili space collection URLs"
```

---

### Task 3: Playlist expand helpers (`playlist.rs`)

**Files:**
- Create: `src-tauri/src/playlist.rs`
- Modify: `src-tauri/src/lib.rs` — add `mod playlist;`

- [ ] **Step 1: Add failing tests** in `playlist.rs`

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_flat_playlist_lines() {
        let out = "BV1aZehzCEJn\t第一集\nBV18fajzfETa\tNA\n\n";
        let items = parse_flat_playlist_output(out);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].id, "BV1aZehzCEJn");
        assert_eq!(items[0].title, "第一集");
        assert_eq!(items[1].id, "BV18fajzfETa");
        assert_eq!(items[1].title, "BV18fajzfETa"); // NA / empty → id
    }

    #[test]
    fn video_url_from_id() {
        assert_eq!(
            bilibili_video_url("BV1xx"),
            "https://www.bilibili.com/video/BV1xx"
        );
    }
}
```

- [ ] **Step 2: Run — expect FAIL** (module missing)

```bash
cd /home/simon/codes/video-downloader/src-tauri && cargo test parse_flat_playlist_lines video_url_from_id -- --nocapture
```

- [ ] **Step 3: Implement `playlist.rs`**

```rust
use crate::download;
use crate::settings::Settings;
use crate::site::{self, Site};
use serde::Serialize;
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Serialize)]
pub struct PlaylistItem {
    pub id: String,
    pub title: String,
}

pub fn bilibili_video_url(id: &str) -> String {
    format!("https://www.bilibili.com/video/{id}")
}

pub fn parse_flat_playlist_output(stdout: &str) -> Vec<PlaylistItem> {
    let mut items = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let (id, title_raw) = match line.split_once('\t') {
            Some((id, title)) => (id.trim(), title.trim()),
            None => (line, ""),
        };
        if id.is_empty() {
            continue;
        }
        let title = if title_raw.is_empty() || title_raw.eq_ignore_ascii_case("NA") {
            id.to_string()
        } else {
            title_raw.to_string()
        };
        items.push(PlaylistItem {
            id: id.to_string(),
            title,
        });
    }
    items
}

/// Run yt-dlp --flat-playlist and return items. Applies Bilibili proxy from settings.
pub fn expand_playlist(url: &str, settings: &Settings) -> Result<Vec<PlaylistItem>, String> {
    let (bin, prefix) = download::resolve_ytdlp()?;
    let mut cmd = Command::new(&bin);
    for p in &prefix {
        cmd.arg(p);
    }
    cmd.arg("--flat-playlist")
        .arg("--print")
        .arg("%(id)s\t%(title)s")
        .arg(url);
    let site = site::detect_site(url);
    if let Some(proxy) = site::proxy_for(site, settings) {
        cmd.arg("--proxy").arg(proxy);
    }
    // YouTube JS runtime not required for Bilibili flat list; harmless if present later.
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let output = cmd
        .output()
        .map_err(|e| format!("展开合集失败（启动 yt-dlp）: {e}"))?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "展开合集失败: {} {}",
            output.status,
            err.chars().take(400).collect::<String>()
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let items = parse_flat_playlist_output(&stdout);
    if items.is_empty() {
        return Err("合集为空或无法解析条目".into());
    }
    let _ = Site::Bilibili; // keep site import meaningful if unused otherwise — remove if clippy complains
    Ok(items)
}
```

Register `mod playlist;` in `lib.rs`. Export `resolve_ytdlp` is already `pub` in `download.rs`.

Remove the dummy `let _ = Site::Bilibili` — only use `site` for `proxy_for`.

- [ ] **Step 4: Run unit tests — expect PASS**

```bash
cd /home/simon/codes/video-downloader/src-tauri && cargo test parse_flat_playlist_lines video_url_from_id -- --nocapture
```

- [ ] **Step 5: Optional live smoke (network)**

```bash
cd /home/simon/codes/video-downloader/src-tauri && cargo test -- --ignored --nocapture
```

Only if you add an `#[ignore]` integration test; otherwise skip and verify later manually with:

```bash
yt-dlp --flat-playlist --print "%(id)s\t%(title)s" "https://space.bilibili.com/7504289/lists/6254946?type=season" | head
```

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/playlist.rs src-tauri/src/lib.rs
git commit -m "feat: expand Bilibili collections via yt-dlp flat-playlist"
```

---

### Task 4: Multi-PID tracking for Stop

**Files:**
- Modify: `src-tauri/src/download.rs`

- [ ] **Step 1: Replace single PID with a set**

Change:

```rust
static CHILD_PID: Mutex<Option<u32>> = Mutex::new(None);
```

to:

```rust
use std::collections::HashSet;

static CHILD_PIDS: Mutex<HashSet<u32>> = Mutex::new(HashSet::new());
```

Replace helpers:

```rust
fn clear_child_pids() {
    if let Ok(mut g) = CHILD_PIDS.lock() {
        g.clear();
    }
}

fn add_child_pid(pid: u32) {
    if let Ok(mut g) = CHILD_PIDS.lock() {
        g.insert(pid);
    }
}

fn remove_child_pid(pid: u32) {
    if let Ok(mut g) = CHILD_PIDS.lock() {
        g.remove(&pid);
    }
}

fn kill_pid(pid: u32) {
    let _ = Command::new("kill")
        .args(["-TERM", "--", &format!("-{pid}")])
        .status();
    let _ = Command::new("kill")
        .args(["-TERM", "--", &pid.to_string()])
        .status();
    std::thread::sleep(Duration::from_millis(300));
    let _ = Command::new("kill")
        .args(["-KILL", "--", &format!("-{pid}")])
        .status();
    let _ = Command::new("kill")
        .args(["-KILL", "--", &pid.to_string()])
        .status();
}
```

Update `stop_download`:

```rust
pub fn stop_download() -> Result<(), String> {
    if !DOWNLOAD_RUNNING.load(Ordering::SeqCst) {
        return Err("当前没有下载任务".into());
    }
    DOWNLOAD_CANCELLED.store(true, Ordering::SeqCst);
    let pids: Vec<u32> = CHILD_PIDS
        .lock()
        .ok()
        .map(|g| g.iter().copied().collect())
        .unwrap_or_default();
    for pid in pids {
        kill_pid(pid);
    }
    Ok(())
}
```

In `run_download` after spawn: `add_child_pid(child.id());`  
After wait: `remove_child_pid(child.id());` (do **not** clear the whole set — other workers may still run).

In `start_download` thread teardown: call `clear_child_pids()` only when the whole session ends (single or batch).

Rename old `clear_child_pid` / `set_child_pid` call sites accordingly.

- [ ] **Step 2: Compile check**

```bash
cd /home/simon/codes/video-downloader/src-tauri && cargo test -- --nocapture
```

Expected: PASS (existing tests).

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/download.rs
git commit -m "fix: track multiple yt-dlp PIDs so stop kills all workers"
```

---

### Task 5: Batch events + concurrent runner

**Files:**
- Modify: `src-tauri/src/download.rs`
- Modify: `src/types.ts`

- [ ] **Step 1: Add event structs** (Rust)

```rust
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchItemMeta {
    pub id: String,
    pub title: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadBatchStarted {
    pub total: usize,
    pub items: Vec<BatchItemMeta>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadItemStarted {
    pub index: usize,
    pub id: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadItemFinished {
    pub index: usize,
    pub id: String,
    pub path: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadItemError {
    pub index: usize,
    pub id: String,
    pub message: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadBatchFinished {
    pub succeeded: usize,
    pub failed: usize,
    pub cancelled: usize,
}
```

Optionally extend `DownloadProgress` with:

```rust
#[serde(skip_serializing_if = "Option::is_none")]
pub item_index: Option<usize>,
#[serde(skip_serializing_if = "Option::is_none")]
pub item_id: Option<String>,
```

For v1, leaving progress without item fields is OK (UI uses latest progress). Prefer adding optional fields defaulting to `None` in single-video emits so TS can ignore.

- [ ] **Step 2: Implement `run_batch_download`**

Sketch (place after `run_download`):

```rust
fn run_batch_download(
    app: &AppHandle,
    page_url: &str,
    category: &str,
    quality: &str,
    audio_only: bool,
) -> Result<(), String> {
    let settings = settings::load_settings(app)?;
    let items = crate::playlist::expand_playlist(page_url, &settings)?;
    let total = items.len();
    let _ = app.emit(
        "download-batch-started",
        DownloadBatchStarted {
            total,
            items: items
                .iter()
                .map(|i| BatchItemMeta {
                    id: i.id.clone(),
                    title: i.title.clone(),
                })
                .collect(),
        },
    );

    let concurrency = settings::clamp_max_concurrent(settings.max_concurrent_downloads) as usize;
    let next = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let succeeded = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let failed = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let started_flags = Arc::new(Mutex::new(vec![false; total]));
    let items = Arc::new(items);

    let worker_count = concurrency.max(1).min(total);
    let mut handles = Vec::new();

    for _ in 0..worker_count {
        let app = app.clone();
        let next = next.clone();
        let items = items.clone();
        let succeeded = succeeded.clone();
        let failed = failed.clone();
        let started_flags = started_flags.clone();
        let category = category.to_string();
        let quality = quality.to_string();
        handles.push(std::thread::spawn(move || {
            loop {
                if DOWNLOAD_CANCELLED.load(Ordering::SeqCst) {
                    break;
                }
                let index = next.fetch_add(1, Ordering::SeqCst);
                if index >= items.len() {
                    break;
                }
                if let Ok(mut flags) = started_flags.lock() {
                    flags[index] = true;
                }
                let item = &items[index];
                let _ = app.emit(
                    "download-item-started",
                    DownloadItemStarted {
                        index,
                        id: item.id.clone(),
                    },
                );
                let url = crate::playlist::bilibili_video_url(&item.id);
                match run_download(&app, &url, &category, &quality, audio_only) {
                    Ok(path) => {
                        succeeded.fetch_add(1, Ordering::SeqCst);
                        let _ = app.emit(
                            "download-item-finished",
                            DownloadItemFinished {
                                index,
                                id: item.id.clone(),
                                path: path.to_string_lossy().into_owned(),
                            },
                        );
                    }
                    Err(message) => {
                        if DOWNLOAD_CANCELLED.load(Ordering::SeqCst)
                            && message.contains("已停止")
                        {
                            // count as cancelled later via not-succeeded path
                            break;
                        }
                        failed.fetch_add(1, Ordering::SeqCst);
                        let _ = app.emit(
                            "download-item-error",
                            DownloadItemError {
                                index,
                                id: item.id.clone(),
                                message,
                            },
                        );
                    }
                }
            }
        }));
    }

    for h in handles {
        let _ = h.join();
    }

    let succeeded_n = succeeded.load(Ordering::SeqCst);
    let failed_n = failed.load(Ordering::SeqCst);
    let flags = started_flags.lock().ok();
    let cancelled_n = if DOWNLOAD_CANCELLED.load(Ordering::SeqCst) {
        total.saturating_sub(succeeded_n + failed_n)
    } else {
        0
    };
    let _ = flags; // unused except for clarity; cancelled = never finished when stop pressed

    let _ = app.emit(
        "download-batch-finished",
        DownloadBatchFinished {
            succeeded: succeeded_n,
            failed: failed_n,
            cancelled: cancelled_n,
        },
    );
    Ok(())
}
```

Refine cancel counting: when stop mid-item, `run_download` returns 「已停止」— treat that item as cancelled (do not increment `failed`). Example:

```rust
Err(message) => {
    if message.contains("已停止") {
        // leave for cancelled tally
    } else {
        failed.fetch_add(1, Ordering::SeqCst);
        emit item-error...
    }
}
```

- [ ] **Step 3: Branch `start_download`**

Inside the spawned thread:

```rust
let result = if site::is_bilibili_collection_url(&url) {
    run_batch_download(&app_for_job, &url, &category, &quality, audio_only)
        .map(|_| PathBuf::new()) // batch uses batch-finished, not download-finished
} else {
    run_download(&app_for_job, &url, &category, &quality, audio_only).map(|p| p)
};

clear_child_pids();
DOWNLOAD_RUNNING.store(false, Ordering::SeqCst);

match result {
    Ok(path) if site::is_bilibili_collection_url(&url) => {
        // batch already emitted download-batch-finished; do NOT emit download-finished
    }
    Ok(path) => {
        let _ = app_for_job.emit("download-finished", DownloadFinished { path: ... });
    }
    Err(message) => {
        let _ = app_for_job.emit("download-error", DownloadError { message });
    }
}
```

Cleaner: change return type of the thread body:

```rust
enum SessionOutcome {
    Single(PathBuf),
    Batch,
}

let outcome = if site::is_bilibili_collection_url(&url) {
    run_batch_download(...)?;
    SessionOutcome::Batch
} else {
    SessionOutcome::Single(run_download(...)?)
};
```

On expand failure inside `run_batch_download`, return `Err` so `download-error` fires (no batch-finished).

- [ ] **Step 4: TS types**

```ts
export type BatchItemMeta = { id: string; title: string };
export type DownloadBatchStarted = { total: number; items: BatchItemMeta[] };
export type DownloadItemStarted = { index: number; id: string };
export type DownloadItemFinished = { index: number; id: string; path: string };
export type DownloadItemError = { index: number; id: string; message: string };
export type DownloadBatchFinished = {
  succeeded: number;
  failed: number;
  cancelled: number;
};
```

- [ ] **Step 5: Compile**

```bash
cd /home/simon/codes/video-downloader/src-tauri && cargo test -- --nocapture
```

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/download.rs src/types.ts
git commit -m "feat: concurrent Bilibili collection batch download with item events"
```

---

### Task 6: Settings UI — concurrent downloads

**Files:**
- Modify: `src/views/SettingsView.tsx`

- [ ] **Step 1: Add form field** after「Bilibili 也走代理」

```tsx
import { InputNumber } from "antd";

<Form.Item
  label="同时下载数"
  extra="合集批量时最多并行几个任务（1–10，默认 5）"
>
  <InputNumber
    min={1}
    max={10}
    value={settings.max_concurrent_downloads}
    onChange={(v) =>
      setSettings({
        ...settings,
        max_concurrent_downloads: typeof v === "number" ? v : 5,
      })
    }
  />
</Form.Item>
```

Ensure loaded settings always have the field (backend default handles old JSON).

- [ ] **Step 2: Typecheck**

```bash
cd /home/simon/codes/video-downloader && npx tsc --noEmit
```

Expected: PASS (or only pre-existing issues).

- [ ] **Step 3: Commit**

```bash
git add src/views/SettingsView.tsx
git commit -m "feat: settings UI for max concurrent downloads"
```

---

### Task 7: DownloadView — task table (Xunlei-like)

**Files:**
- Modify: `src/views/DownloadView.tsx`

- [ ] **Step 1: Add task state types**

```tsx
type TaskStatus = "pending" | "downloading" | "done" | "failed" | "cancelled";

type TaskRow = {
  index: number;
  id: string;
  title: string;
  status: TaskStatus;
  detail: string;
};
```

State:

```tsx
const [tasks, setTasks] = useState<TaskRow[]>([]);
const [batchMode, setBatchMode] = useState(false);
const [batchSummary, setBatchSummary] = useState<string | null>(null);
```

- [ ] **Step 2: Wire listeners** (same `useEffect` as existing download events)

```tsx
const uBatchStart = await listen<DownloadBatchStarted>("download-batch-started", (e) => {
  if (cancelled) return;
  setBatchMode(true);
  setTasks(
    e.payload.items.map((it, index) => ({
      index,
      id: it.id,
      title: it.title,
      status: "pending" as const,
      detail: "",
    }))
  );
  setBatchSummary(`共 ${e.payload.total} · 完成 0 · 失败 0 · 进行中 0`);
});

const uItemStart = await listen<DownloadItemStarted>("download-item-started", (e) => {
  setTasks((prev) =>
    prev.map((t) =>
      t.index === e.payload.index ? { ...t, status: "downloading", detail: "" } : t
    )
  );
});

const uItemDone = await listen<DownloadItemFinished>("download-item-finished", (e) => {
  setTasks((prev) =>
    prev.map((t) =>
      t.index === e.payload.index
        ? { ...t, status: "done", detail: e.payload.path }
        : t
    )
  );
});

const uItemErr = await listen<DownloadItemError>("download-item-error", (e) => {
  setTasks((prev) =>
    prev.map((t) =>
      t.index === e.payload.index
        ? { ...t, status: "failed", detail: e.payload.message }
        : t
    )
  );
});

const uBatchEnd = await listen<DownloadBatchFinished>("download-batch-finished", (e) => {
  setBusy(false);
  setSpeed(null);
  setEta(null);
  setTasks((prev) =>
    prev.map((t) =>
      t.status === "pending" || t.status === "downloading"
        ? { ...t, status: "cancelled", detail: t.detail || "已取消" }
        : t
    )
  );
  setBatchSummary(
    `共完成 ${e.payload.succeeded} · 失败 ${e.payload.failed} · 取消 ${e.payload.cancelled}`
  );
  setPercent(100);
  onCategoriesChangedRef.current();
});
```

Update summary counts whenever tasks change (derive in render):

```tsx
const done = tasks.filter((t) => t.status === "done").length;
const failed = tasks.filter((t) => t.status === "failed").length;
const active = tasks.filter((t) => t.status === "downloading").length;
const summaryText = batchMode
  ? `共 ${tasks.length} · 完成 ${done} · 失败 ${failed} · 进行中 ${active}`
  : null;
```

On `onStart`: clear `tasks`, `batchMode`, `batchSummary`.

On single-video `download-finished` / `download-error`: if `!batchMode`, keep current behavior; if somehow batchMode, ignore single finished.

Status label map:

```tsx
const statusLabel: Record<TaskStatus, string> = {
  pending: "等待",
  downloading: "下载中",
  done: "完成",
  failed: "失败",
  cancelled: "已取消",
};
```

- [ ] **Step 3: Render Table**

Import `Table` from antd. Below Progress:

```tsx
{batchMode && tasks.length > 0 && (
  <>
    <Typography.Paragraph type="secondary">{summaryText}</Typography.Paragraph>
    <Table
      size="small"
      pagination={false}
      rowKey="id"
      dataSource={tasks}
      scroll={{ y: 320 }}
      columns={[
        { title: "#", dataIndex: "index", width: 56, render: (i: number) => i + 1 },
        { title: "标题", dataIndex: "title", ellipsis: true },
        {
          title: "状态",
          dataIndex: "status",
          width: 88,
          render: (s: TaskStatus) => statusLabel[s],
        },
        {
          title: "详情",
          dataIndex: "detail",
          ellipsis: true,
          render: (d: string, row: TaskRow) =>
            row.status === "failed" ? (
              <Typography.Text type="danger" ellipsis={{ tooltip: d }}>
                {d}
              </Typography.Text>
            ) : row.status === "done" ? (
              <Typography.Text type="secondary" ellipsis={{ tooltip: d }}>
                已保存
              </Typography.Text>
            ) : (
              d
            ),
        },
      ]}
    />
  </>
)}
```

Success Alert for single video unchanged; for batch, prefer summary Alert after `download-batch-finished` when `failed > 0` use `warning`, else `success`.

Placeholder on URL input: `粘贴 YouTube / Bilibili 链接（支持合集）`

- [ ] **Step 4: Typecheck**

```bash
cd /home/simon/codes/video-downloader && npx tsc --noEmit
```

- [ ] **Step 5: Commit**

```bash
git add src/views/DownloadView.tsx src/types.ts
git commit -m "feat: Xunlei-like task table for collection batch downloads"
```

---

### Task 8: README + manual verification

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Document**

Under「使用」add:

- 支持 Bilibili 合集链接（`space.bilibili.com/.../lists/...?type=season`）：展开后批量下载，下载页显示每集状态。
- 设置中可配置「同时下载数」（默认 5）。

- [ ] **Step 2: Manual test checklist** (run `npm run tauri dev`)

1. Paste `https://space.bilibili.com/7504289/lists/6254946?type=season` → task table appears with many rows.
2. Default concurrency 5 → up to ~5 rows show「下载中」.
3. Stop mid-batch → active stop, pending →「已取消」.
4. Single BV URL still one-shot, no table.
5. Settings → set concurrent to 1 → save → restart batch → strictly serial.
6. (Optional) Disconnect mid-item → that row「失败」, others continue.

- [ ] **Step 3: Commit**

```bash
git add README.md
git commit -m "docs: document Bilibili collection batch download"
```

---

### Spec coverage checklist

| Spec requirement | Task |
|------------------|------|
| Collection URL detect | Task 2 |
| flat-playlist expand | Task 3 |
| Serializable concurrent queue default 5 | Tasks 1, 5, 6 |
| Per-item status / fail continue | Tasks 5, 7 |
| Stop kills all + cancel pending | Tasks 4, 5, 7 |
| Single video unchanged | Task 5 branch |
| Settings UI 1–10 | Tasks 1, 6 |
| No collection subfolder | Task 5 uses same `run_download` paths |
| Verification criteria | Task 8 |

### Placeholder / consistency review

- Event names match spec (`download-batch-started`, `download-item-*`, `download-batch-finished`).
- Setting name `max_concurrent_downloads` consistent Rust ↔ TS (serde camelCase not used on Settings today — field is snake_case in JSON already for other keys; keep snake_case).
- Batch item events use `camelCase` via serde rename for TS ergonomics; Settings remain snake_case like existing API.

---

### Execution handoff

Plan complete and saved to `docs/superpowers/plans/2026-09-20-bilibili-season-batch.md`.

**Two execution options:**

1. **Subagent-Driven (recommended)** — fresh subagent per task, review between tasks  
2. **Inline Execution** — execute tasks in this session with executing-plans checkpoints  

Which approach?
