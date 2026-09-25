# Download Queue Resume Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Persist download sessions to `download_queue.json` so after crash/restart the user can Continue or Discard; resume unfinished + failed items with yt-dlp `--continue`.

**Architecture:** New `queue.rs` owns load/save/clear and resumable checks. `download.rs` writes the queue on session start, updates item statuses as they progress, clears on complete/stop/discard. Batch resume reuses the worker pool but skips `done` items. Frontend shows a Modal on boot via `get_download_queue`.

**Tech Stack:** Tauri 2 + Rust (serde JSON), React + Ant Design Modal

**Spec:** `docs/superpowers/specs/2026-09-20-download-queue-resume-design.md`

---

### File map

| File | Responsibility |
|------|----------------|
| `src-tauri/src/queue.rs` | Queue types, path, load/save/clear, resumable filter, temp-file cleanup helpers |
| `src-tauri/src/download.rs` | `--continue`; create/update/clear queue; batch skip-done; resume entrypoint; stop clears queue |
| `src-tauri/src/lib.rs` | Register `get_download_queue` / `resume_download_queue` / `discard_download_queue` |
| `src/types.ts` | TS types for queue |
| `src/api.ts` | Invoke wrappers |
| `src/App.tsx` | Boot Modal Continue / Discard |
| `src/views/DownloadView.tsx` | Optional: accept resume kickoff / hydrate table from queue snapshot |
| `README.md` | Mention resume behavior |

---

### Task 1: Queue module — types + load/save/clear (TDD)

**Files:**
- Create: `src-tauri/src/queue.rs`
- Modify: `src-tauri/src/lib.rs` — `mod queue;`

- [ ] **Step 1: Add failing tests** in `queue.rs`

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_queue_path() -> PathBuf {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("videofetch_queue_test_{n}.json"))
    }

    fn sample_batch() -> DownloadQueue {
        DownloadQueue {
            version: 1,
            kind: QueueKind::Batch,
            page_url: "https://space.bilibili.com/1/lists/2?type=season".into(),
            category: "测试".into(),
            quality: "720".into(),
            audio_only: false,
            updated_at: "2026-09-20T00:00:00Z".into(),
            items: vec![
                QueueItem {
                    index: 0,
                    id: "BV1".into(),
                    title: "一".into(),
                    url: "https://www.bilibili.com/video/BV1".into(),
                    status: ItemStatus::Done,
                },
                QueueItem {
                    index: 1,
                    id: "BV2".into(),
                    title: "二".into(),
                    url: "https://www.bilibili.com/video/BV2".into(),
                    status: ItemStatus::Pending,
                },
            ],
        }
    }

    #[test]
    fn roundtrip_save_load() {
        let path = temp_queue_path();
        let q = sample_batch();
        save_queue_to(&path, &q).unwrap();
        let back = load_queue_from(&path).unwrap().unwrap();
        assert_eq!(back.kind, QueueKind::Batch);
        assert_eq!(back.items.len(), 2);
        assert_eq!(back.items[1].status, ItemStatus::Pending);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn is_resumable_requires_incomplete() {
        let mut q = sample_batch();
        assert!(q.is_resumable());
        q.items[1].status = ItemStatus::Done;
        assert!(!q.is_resumable());
        q.items[1].status = ItemStatus::Failed;
        assert!(q.is_resumable());
    }

    #[test]
    fn corrupt_or_missing_returns_none() {
        let path = temp_queue_path();
        assert!(load_queue_from(&path).unwrap().is_none());
        fs::write(&path, "{not json").unwrap();
        assert!(load_queue_from(&path).unwrap().is_none());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn clear_removes_file() {
        let path = temp_queue_path();
        save_queue_to(&path, &sample_batch()).unwrap();
        clear_queue_at(&path).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn set_item_status_updates() {
        let mut q = sample_batch();
        q.set_item_status(1, ItemStatus::Downloading);
        assert_eq!(q.items[1].status, ItemStatus::Downloading);
    }
}
```

- [ ] **Step 2: Run — expect FAIL**

```bash
cd /home/simon/codes/video-downloader/src-tauri && cargo test queue:: -- --nocapture
```

Expected: module / types missing.

- [ ] **Step 3: Implement `queue.rs`**

```rust
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueKind {
    Batch,
    Single,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemStatus {
    Pending,
    Downloading,
    Done,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueItem {
    pub index: usize,
    pub id: String,
    pub title: String,
    pub url: String,
    pub status: ItemStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadQueue {
    pub version: u32,
    pub kind: QueueKind,
    pub page_url: String,
    pub category: String,
    pub quality: String,
    pub audio_only: bool,
    pub updated_at: String,
    pub items: Vec<QueueItem>,
}

impl DownloadQueue {
    pub fn is_resumable(&self) -> bool {
        self.items.iter().any(|i| {
            matches!(
                i.status,
                ItemStatus::Pending | ItemStatus::Downloading | ItemStatus::Failed
            )
        })
    }

    pub fn set_item_status(&mut self, index: usize, status: ItemStatus) {
        if let Some(item) = self.items.iter_mut().find(|i| i.index == index) {
            item.status = status;
        }
    }

    pub fn touch(&mut self) {
        self.updated_at = chrono_like_now();
    }
}

fn chrono_like_now() -> String {
    // Avoid new chrono dep: use simple UTC-ish stamp via SystemTime
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}

pub fn queue_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("config dir: {e}"))?;
    Ok(dir.join("download_queue.json"))
}

pub fn load_queue_from(path: &Path) -> Result<Option<DownloadQueue>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let raw = match fs::read_to_string(path) {
        Ok(r) => r,
        Err(e) => return Err(format!("read queue: {e}")),
    };
    match serde_json::from_str::<DownloadQueue>(&raw) {
        Ok(q) => Ok(Some(q)),
        Err(e) => {
            eprintln!("download_queue.json corrupt, ignoring: {e}");
            Ok(None)
        }
    }
}

pub fn save_queue_to(path: &Path, queue: &DownloadQueue) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create config dir: {e}"))?;
    }
    let raw = serde_json::to_string_pretty(queue).map_err(|e| e.to_string())?;
    fs::write(path, raw).map_err(|e| format!("write queue: {e}"))
}

pub fn clear_queue_at(path: &Path) -> Result<(), String> {
    if path.exists() {
        fs::remove_file(path).map_err(|e| format!("remove queue: {e}"))?;
    }
    Ok(())
}

pub fn load_queue(app: &AppHandle) -> Result<Option<DownloadQueue>, String> {
    load_queue_from(&queue_path(app)?)
}

pub fn save_queue(app: &AppHandle, queue: &DownloadQueue) -> Result<(), String> {
    save_queue_to(&queue_path(app)?, queue)
}

pub fn clear_queue(app: &AppHandle) -> Result<(), String> {
    clear_queue_at(&queue_path(app)?)
}

/// Returns queue only if resumable; otherwise None (and optionally clear all-done file).
pub fn load_resumable_queue(app: &AppHandle) -> Result<Option<DownloadQueue>, String> {
    match load_queue(app)? {
        Some(q) if q.is_resumable() => Ok(Some(q)),
        Some(_) => {
            let _ = clear_queue(app);
            Ok(None)
        }
        None => Ok(None),
    }
}
```

Register `mod queue;` in `lib.rs`.

**Note:** Prefer `updated_at` as unix-seconds string above to avoid adding `chrono`. Spec shows ISO-8601; either is fine if documented — if you prefer ISO without chrono:

```rust
// still OK to store unix seconds; frontend can display counts without parsing time
```

- [ ] **Step 4: Run tests — expect PASS**

```bash
cd /home/simon/codes/video-downloader/src-tauri && cargo test queue:: -- --nocapture
```

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/queue.rs src-tauri/src/lib.rs
git commit -m "feat: add download_queue.json persistence module"
```

---

### Task 2: Temp-file cleanup helper (TDD)

**Files:**
- Modify: `src-tauri/src/queue.rs`

- [ ] **Step 1: Failing test**

```rust
#[test]
fn cleanup_temp_files_removes_part_and_ytdl_matching_ids() {
    let dir = tempfile::tempdir().unwrap();
    // Prefer std-only to avoid new dep: use std::env::temp_dir + unique subdir
}
```

If `tempfile` crate is not in `Cargo.toml`, **do not add it**. Use:

```rust
#[test]
fn cleanup_temp_files_removes_part_and_ytdl_matching_ids() {
    use std::fs;
    let root = std::env::temp_dir().join(format!(
        "vf_cleanup_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let cat = root.join("分类");
    fs::create_dir_all(&cat).unwrap();
    let part = cat.join("foo [BV2].mp4.part");
    let ytdl = cat.join("foo [BV2].mp4.ytdl");
    let keep = cat.join("foo [BV1].mp4");
    let other = cat.join("bar [BV9].mp4.part");
    fs::write(&part, b"x").unwrap();
    fs::write(&ytdl, b"x").unwrap();
    fs::write(&keep, b"x").unwrap();
    fs::write(&other, b"x").unwrap();

    cleanup_temp_files_in_category(&cat, &["BV2".into()]).unwrap();

    assert!(!part.exists());
    assert!(!ytdl.exists());
    assert!(keep.exists());
    assert!(other.exists()); // different id — leave alone
    let _ = fs::remove_dir_all(&root);
}
```

- [ ] **Step 2: Run — expect FAIL**

```bash
cd /home/simon/codes/video-downloader/src-tauri && cargo test cleanup_temp_files -- --nocapture
```

- [ ] **Step 3: Implement**

```rust
/// Remove `.part` / `.ytdl` (and similar) whose filename contains `[id]` for given ids.
pub fn cleanup_temp_files_in_category(category_dir: &Path, ids: &[String]) -> Result<(), String> {
    if !category_dir.is_dir() {
        return Ok(());
    }
    let entries = fs::read_dir(category_dir).map_err(|e| e.to_string())?;
    for ent in entries.flatten() {
        let name = ent.file_name().to_string_lossy().into_owned();
        let lower = name.to_lowercase();
        let is_temp = lower.ends_with(".part")
            || lower.ends_with(".ytdl")
            || lower.contains(".part.")
            || looks_like_incomplete(&name);
        if !is_temp {
            continue;
        }
        let matched = ids.iter().any(|id| name.contains(&format!("[{id}]")));
        if matched {
            let _ = fs::remove_file(ent.path());
        }
    }
    Ok(())
}

fn looks_like_incomplete(name: &str) -> bool {
    // yt-dlp fragment leftovers already skipped by library; also catch `.f123.mp4` mid-merge
    crate::library::looks_like_ytdlp_fragment(name)
}
```

For discard: only clean ids that are **not** `Done` (pending/downloading/failed).

```rust
pub fn discard_queue_temps(library_root: &Path, queue: &DownloadQueue) -> Result<(), String> {
    let cat = library_root.join(&queue.category);
    let ids: Vec<String> = queue
        .items
        .iter()
        .filter(|i| i.status != ItemStatus::Done)
        .map(|i| i.id.clone())
        .collect();
    cleanup_temp_files_in_category(&cat, &ids)
}
```

- [ ] **Step 4: Tests PASS + commit**

```bash
cd /home/simon/codes/video-downloader/src-tauri && cargo test cleanup_temp_files -- --nocapture
git add src-tauri/src/queue.rs
git commit -m "feat: cleanup yt-dlp temp files when discarding queue"
```

---

### Task 3: Add `--continue` to every download

**Files:**
- Modify: `src-tauri/src/download.rs` (`run_download` command builder)

- [ ] **Step 1: Add `--continue`** next to `--newline` / `--no-playlist`:

```rust
cmd.arg("--newline")
    .arg("--no-playlist")
    .arg("--continue")
    .arg("--progress");
```

- [ ] **Step 2: Compile check**

```bash
cd /home/simon/codes/video-downloader/src-tauri && cargo test -- --nocapture
```

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/download.rs
git commit -m "feat: pass --continue to yt-dlp for resume support"
```

---

### Task 4: Persist queue during download sessions

**Files:**
- Modify: `src-tauri/src/download.rs`
- Modify: `src-tauri/src/download.rs` `stop_download`

- [ ] **Step 1: Helper to persist status updates**

```rust
fn persist_item_status(app: &AppHandle, index: usize, status: queue::ItemStatus) {
    if let Ok(Some(mut q)) = queue::load_queue(app) {
        q.set_item_status(index, status);
        q.touch();
        let _ = queue::save_queue(app, &q);
    }
}

fn persist_new_queue(app: &AppHandle, q: queue::DownloadQueue) {
    let _ = queue::save_queue(app, &q);
}
```

- [ ] **Step 2: Single-video path in `start_download` thread**

Before `run_download` for single:

```rust
let single_queue = queue::DownloadQueue {
    version: 1,
    kind: queue::QueueKind::Single,
    page_url: url.clone(),
    category: category.clone(),
    quality: quality.clone(),
    audio_only,
    updated_at: String::new(),
    items: vec![queue::QueueItem {
        index: 0,
        id: url.clone(), // or extract id later; title can be url for now
        title: url.clone(),
        url: url.clone(),
        status: queue::ItemStatus::Downloading,
    }],
};
let mut sq = single_queue;
sq.touch();
persist_new_queue(&app_for_job, sq);
```

On single success: `queue::clear_queue(&app)`.  
On single error (not stop): set item `Failed` then leave queue (resumable) **OR** clear — Spec: crash mid-download should resume. For terminal failure after yt-dlp exits non-zero, keep as `failed` so Continue retries. On stop: clear (see Step 4).

```rust
match run_download(...) {
    Ok(path) => {
        let _ = queue::clear_queue(&app_for_job);
        Ok(SessionOutcome::Single(path))
    }
    Err(message) if message.contains("已停止") => {
        let _ = queue::clear_queue(&app_for_job);
        Err(message)
    }
    Err(message) => {
        persist_item_status(&app_for_job, 0, queue::ItemStatus::Failed);
        Err(message)
    }
}
```

- [ ] **Step 3: Batch path — write queue after expand, update per item**

Refactor `run_batch_download` to accept optional prebuilt items. After expand:

```rust
let queue_items: Vec<queue::QueueItem> = items
    .iter()
    .enumerate()
    .map(|(index, i)| queue::QueueItem {
        index,
        id: i.id.clone(),
        title: i.title.clone(),
        url: crate::playlist::bilibili_video_url(&i.id),
        status: queue::ItemStatus::Pending,
    })
    .collect();
let mut q = queue::DownloadQueue {
    version: 1,
    kind: queue::QueueKind::Batch,
    page_url: page_url.to_string(),
    category: category.to_string(),
    quality: quality.to_string(),
    audio_only,
    updated_at: String::new(),
    items: queue_items,
};
q.touch();
persist_new_queue(app, q);
```

In worker, before download: `persist_item_status(app, index, Downloading)`.  
On Ok: `persist_item_status(..., Done)`.  
On Err (not stop): `persist_item_status(..., Failed)`.

After batch finishes **without** cancel and with `failed_n == 0`: `clear_queue`.  
If any failed remain: **keep** queue (resumable).  
If cancelled (user stop): `clear_queue` (stop handler also clears — idempotent).

Spec: "全部完成 → 删除". Partial failures keep file so Continue retries failures.

```rust
if DOWNLOAD_CANCELLED.load(...) {
    let _ = queue::clear_queue(app);
} else if failed_n == 0 {
    let _ = queue::clear_queue(app);
}
// else keep queue with failed/pending
```

- [ ] **Step 4: `stop_download` clears queue**

```rust
pub fn stop_download(app: AppHandle) -> Result<(), String> {
    // existing kill logic...
    let _ = queue::clear_queue(&app);
    Ok(())
}
```

Update `lib.rs` to pass `AppHandle` into `stop_download`:

```rust
fn stop_download(app: tauri::AppHandle) -> Result<(), String> {
    download::stop_download(app)
}
```

- [ ] **Step 5: Compile + commit**

```bash
cd /home/simon/codes/video-downloader/src-tauri && cargo test -- --nocapture
git add src-tauri/src/download.rs src-tauri/src/lib.rs
git commit -m "feat: persist download queue during single and batch sessions"
```

---

### Task 5: Resume batch/single from queue

**Files:**
- Modify: `src-tauri/src/download.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Generalize batch runner**

Change signature:

```rust
fn run_batch_download(
    app: &AppHandle,
    page_url: &str,
    category: &str,
    quality: &str,
    audio_only: bool,
    resume_items: Option<Vec<queue::QueueItem>>,
) -> Result<(), String>
```

Logic:

```rust
let (playlist_meta, work_indices) = if let Some(items) = resume_items {
    // Emit batch-started from queue items (all rows for UI)
    let meta: Vec<BatchItemMeta> = items
        .iter()
        .map(|i| BatchItemMeta {
            id: i.id.clone(),
            title: i.title.clone(),
        })
        .collect();
    let indices: Vec<usize> = items
        .iter()
        .filter(|i| i.status != queue::ItemStatus::Done)
        .map(|i| i.index)
        .collect();
    // Build Arc of urls/titles keyed by index from items
    (meta, indices, items)
} else {
    // existing expand path...
};
```

Worker pool should pull from `work_indices` (a shared queue of indices to process), not `0..total`. For resume, skip done.

Simpler approach: keep `items: Arc<Vec<PlaylistItem>>` aligned by index, and a `Vec<usize>` of pending indices consumed via `AtomicUsize` into that list:

```rust
let todo: Arc<Vec<usize>> = Arc::new(work_indices);
let next = AtomicUsize::new(0);
// in worker:
let pos = next.fetch_add(1, SeqCst);
if pos >= todo.len() { break; }
let index = todo[pos];
let item = &items[index];
```

When `resume_items` is `Some`, **do not** re-expand playlist; rebuild `items` from queue (`id`/`title`). Still emit `download-batch-started` with **all** items so UI shows done rows.

When fresh start, write queue as in Task 4 after expand.

- [ ] **Step 2: `resume_download_queue`**

```rust
pub fn resume_download_queue(app: AppHandle) -> Result<(), String> {
    let q = queue::load_resumable_queue(&app)?
        .ok_or_else(|| "没有可恢复的下载".to_string())?;
    if DOWNLOAD_RUNNING.swap(true, Ordering::SeqCst) {
        return Err("已有下载任务在进行".into());
    }
    DOWNLOAD_CANCELLED.store(false, Ordering::SeqCst);

    let app_for_job = app.clone();
    std::thread::spawn(move || {
        let outcome = (|| -> Result<SessionOutcome, String> {
            match q.kind {
                queue::QueueKind::Batch => {
                    run_batch_download(
                        &app_for_job,
                        &q.page_url,
                        &q.category,
                        &q.quality,
                        q.audio_only,
                        Some(q.items.clone()),
                    )?;
                    Ok(SessionOutcome::Batch)
                }
                queue::QueueKind::Single => {
                    let item = q.items.first().ok_or("队列为空")?;
                    // reset status downloading
                    persist_item_status(&app_for_job, 0, queue::ItemStatus::Downloading);
                    let path = run_download(
                        &app_for_job,
                        &item.url,
                        &q.category,
                        &q.quality,
                        q.audio_only,
                        true,
                        true,
                    )?;
                    let _ = queue::clear_queue(&app_for_job);
                    Ok(SessionOutcome::Single(path))
                }
            }
        })();
        clear_child_pids();
        DOWNLOAD_RUNNING.store(false, Ordering::SeqCst);
        // same emit match as start_download
        match outcome { ... }
    });
    Ok(())
}
```

- [ ] **Step 3: `get_download_queue` / `discard_download_queue` commands**

```rust
// lib.rs
#[tauri::command]
fn get_download_queue(app: tauri::AppHandle) -> Result<Option<queue::DownloadQueue>, String> {
    queue::load_resumable_queue(&app)
}

#[tauri::command]
fn resume_download_queue(app: tauri::AppHandle) -> Result<(), String> {
    download::resume_download_queue(app)
}

#[tauri::command]
fn discard_download_queue(app: tauri::AppHandle) -> Result<(), String> {
    download::discard_download_queue(app)
}
```

```rust
// download.rs
pub fn discard_download_queue(app: AppHandle) -> Result<(), String> {
    if let Some(q) = queue::load_queue(&app)? {
        let settings = settings::load_settings(&app)?;
        let root = settings::library_root_path(&settings);
        let _ = queue::discard_queue_temps(&root, &q);
    }
    queue::clear_queue(&app)
}
```

Register all three in `generate_handler!`.

Ensure `DownloadQueue` / nested types are `Serialize` for the invoke return (already are).

- [ ] **Step 4: Wire fresh `start_download` batch call**

```rust
run_batch_download(&app, &url, &category, &quality, audio_only, None)?;
```

- [ ] **Step 5: Tests compile + commit**

```bash
cd /home/simon/codes/video-downloader/src-tauri && cargo test -- --nocapture
git add src-tauri/src/download.rs src-tauri/src/lib.rs src-tauri/src/queue.rs
git commit -m "feat: resume and discard persisted download queues"
```

---

### Task 6: Frontend types + API

**Files:**
- Modify: `src/types.ts`
- Modify: `src/api.ts`

- [ ] **Step 1: Types**

```ts
export type QueueKind = "batch" | "single";
export type ItemStatus = "pending" | "downloading" | "done" | "failed";

export type QueueItem = {
  index: number;
  id: string;
  title: string;
  url: string;
  status: ItemStatus;
};

export type DownloadQueue = {
  version: number;
  kind: QueueKind;
  page_url: string;
  category: string;
  quality: string;
  audio_only: boolean;
  updated_at: string;
  items: QueueItem[];
};
```

**Serde note:** Rust uses `rename_all = "snake_case"` for enums → JSON `"batch"`, `"pending"`. Struct fields stay snake_case (`page_url`, `audio_only`) matching existing Settings style. TS must use snake_case field names.

- [ ] **Step 2: API**

```ts
import type { DownloadQueue, Settings, VideoItem } from "./types";

getDownloadQueue: () => invoke<DownloadQueue | null>("get_download_queue"),
resumeDownloadQueue: () => invoke<void>("resume_download_queue"),
discardDownloadQueue: () => invoke<void>("discard_download_queue"),
```

- [ ] **Step 3: `npx tsc --noEmit` + commit**

```bash
cd /home/simon/codes/video-downloader && npx tsc --noEmit
git add src/types.ts src/api.ts
git commit -m "feat: expose download queue resume APIs to frontend"
```

---

### Task 7: Boot Modal + DownloadView hydration

**Files:**
- Modify: `src/App.tsx`
- Modify: `src/views/DownloadView.tsx` (hydrate helpers / props)

- [ ] **Step 1: App Modal state**

After `refresh()` on boot:

```tsx
import { Modal } from "antd";
import type { DownloadQueue } from "./types";

const [pendingQueue, setPendingQueue] = useState<DownloadQueue | null>(null);

// in boot effect after refresh:
const q = await api.getDownloadQueue();
if (q) setPendingQueue(q);

function queueSummary(q: DownloadQueue): string {
  const done = q.items.filter((i) => i.status === "done").length;
  const todo = q.items.length - done;
  if (q.kind === "batch") {
    return `合集 · 共 ${q.items.length} · 已完成 ${done} · 待处理 ${todo} · 分类「${q.category}」`;
  }
  const title = q.items[0]?.title || q.page_url;
  return `单视频 · ${title} · 分类「${q.category}」`;
}

async function onResumeQueue() {
  setPendingQueue(null);
  setTab("download");
  // Signal DownloadView — see below
  setResumeToken((n) => n + 1);
  await api.resumeDownloadQueue();
}

async function onDiscardQueue() {
  await api.discardDownloadQueue();
  setPendingQueue(null);
}
```

Modal:

```tsx
<Modal
  title="未完成的下载"
  open={!!pendingQueue}
  okText="继续"
  cancelText="丢弃"
  onOk={onResumeQueue}
  onCancel={onDiscardQueue}
  closable={false}
  maskClosable={false}
>
  {pendingQueue && <p>{queueSummary(pendingQueue)}</p>}
</Modal>
```

**UX note:** Ant Design `onCancel` fires for both Cancel button and X — we set `closable={false}`. Map Cancel button to Discard via `cancelText` + `onCancel={onDiscardQueue}`. Ok = Continue.

- [ ] **Step 2: Hydrate DownloadView before events arrive**

Pass `resumeSeed: DownloadQueue | null` from App when user clicks Continue (keep a copy before clearing modal):

```tsx
const [resumeSeed, setResumeSeed] = useState<DownloadQueue | null>(null);

async function onResumeQueue() {
  if (!pendingQueue) return;
  setResumeSeed(pendingQueue);
  setPendingQueue(null);
  setTab("download");
  setBusyHint(true);
  await api.resumeDownloadQueue();
}
```

In `DownloadView`, new optional props:

```tsx
resumeSeed: DownloadQueue | null;
onResumeSeedConsumed: () => void;
```

`useEffect` when `resumeSeed` set:

```tsx
useEffect(() => {
  if (!resumeSeed) return;
  setBatchMode(true);
  setBusy(true);
  setTasks(
    resumeSeed.items.map((it) => ({
      index: it.index,
      id: it.id,
      title: it.title,
      status:
        it.status === "done"
          ? "done"
          : it.status === "failed"
            ? "pending" // will retry
            : "pending",
      detail: it.status === "done" ? "已保存" : "",
    })),
  );
  // For single: leave batchMode false, set url field optional
  if (resumeSeed.kind === "single") {
    setBatchMode(false);
    setUrl(resumeSeed.page_url);
  }
  onResumeSeedConsumed();
}, [resumeSeed]);
```

When backend emits `download-batch-started` again, it will rebuild the table — that is OK (overwrite with same items). Prefer letting backend event win: seed only sets `busy` + `batchMode` + placeholder rows until event arrives.

Simpler seed behavior:

```tsx
// On resumeSeed: setBusy(true); setBatchMode(resumeSeed.kind === "batch");
// setTasks from seed with done/pending; then clear seed.
// When download-batch-started arrives, replace tasks from payload but MERGE done statuses from previous if needed.
```

Actually backend resume emits `download-batch-started` with all items — frontend listener already builds all-pending table. **Problem:** that loses visual "done" until we fix the event or seed.

**Fix backend event for resume:** include status in `BatchItemMeta` OR emit items then immediately emit synthetic finished for done indices.

Minimal change — extend `BatchItemMeta`:

```rust
pub struct BatchItemMeta {
    pub id: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>, // "done" | "pending" | ...
}
```

Or always send status string. Update TS + DownloadView listener:

```tsx
status: it.status === "done" ? "done" : "pending",
detail: it.status === "done" ? "已保存" : "",
```

Do this in Task 5 when emitting batch-started from resume path (and fresh path status = pending).

- [ ] **Step 3: tsc + commit**

```bash
cd /home/simon/codes/video-downloader && npx tsc --noEmit
git add src/App.tsx src/views/DownloadView.tsx src-tauri/src/download.rs src/types.ts
git commit -m "feat: prompt to resume or discard download queue on startup"
```

---

### Task 8: README + manual verification

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Document**

Under 使用:

- 若上次下载未完成（崩溃或强制退出），启动时会提示「继续 / 丢弃」。继续将跳过已完成项并用 yt-dlp 断点续传；丢弃会清除队列与临时文件。主动点「停止」不会提示恢复。

- [ ] **Step 2: Manual checklist** (report in commit message / leave for human)

1. Start batch, kill app mid-way → relaunch → Modal → Continue → done rows stay done.  
2. Single video kill mid-way → Continue → `--continue`.  
3. Discard → no modal next launch; `.part` gone.  
4. Stop → no modal next launch.  
5. Fail one item, kill app → Continue retries failed.

- [ ] **Step 3: Commit**

```bash
git add README.md
git commit -m "docs: document download queue resume on startup"
```

---

### Spec coverage checklist

| Spec requirement | Task |
|------------------|------|
| `download_queue.json` model | Task 1 |
| Write on start / status / clear on complete | Task 4 |
| Stop clears queue | Task 4 |
| `--continue` | Task 3 |
| get / resume / discard APIs | Task 5–6 |
| Startup Modal Continue/Discard | Task 7 |
| Retry failed on continue | Task 5 (skip only `done`) |
| Temp cleanup on discard | Task 2 + 5 |
| Single + batch | Tasks 4–5 |
| README | Task 8 |

### Type consistency

- Rust enums: `snake_case` in JSON (`batch`, `pending`) ↔ TS string unions.  
- Struct fields: `page_url`, `audio_only` snake_case in both.  
- `stop_download(app)` gains `AppHandle`.  
- `run_batch_download(..., resume_items: Option<Vec<QueueItem>>)`.  
- `BatchItemMeta.status` optional/required string for UI hydration.

### Placeholder scan

No TBD/TODO left in steps above.

---

### Execution handoff

Plan complete and saved to `docs/superpowers/plans/2026-09-20-download-queue-resume.md`.

**Two execution options:**

1. **Subagent-Driven (recommended)** — fresh subagent per task, review between tasks  
2. **Inline Execution** — execute in this session with executing-plans checkpoints  

Which approach?
