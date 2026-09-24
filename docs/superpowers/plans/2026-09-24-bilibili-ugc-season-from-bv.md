# Bilibili ugc_season from BV Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Paste a Bilibili video BV URL that belongs to a `ugc_season`; download every episode’s every 分P into `{category}/{season}/{episode}/`.

**Architecture:** On start, if the URL is a Bilibili video page, fetch `x/web-interface/view` and parse `ugc_season`. Expand to flat batch items (id=`{bvid}_p{N}`, full `?p=` URL, relative `subdir`). Reuse existing batch worker pool; extend `PlaylistItem` / `QueueItem` / `run_download` so each item can carry URL, subdir, and fixed output stem. Make library listing recursive so nested files appear.

**Tech Stack:** Rust / Tauri 2, `ureq` (blocking HTTP), `serde_json`, existing yt-dlp batch download path.

**Spec:** `docs/superpowers/specs/2026-09-24-bilibili-ugc-season-from-bv-design.md`

---

## File map

| File | Role |
|---|---|
| Create: `src-tauri/src/bilibili.rs` | BV extract, filename sanitize, view JSON parse, HTTP fetch, expand season → items |
| Modify: `src-tauri/src/lib.rs` | `mod bilibili;` |
| Modify: `src-tauri/Cargo.toml` | add `ureq` with `json` |
| Modify: `src-tauri/src/playlist.rs` | extend `PlaylistItem` with `url` + `subdir` + `output_stem` |
| Modify: `src-tauri/src/queue.rs` | `QueueItem.subdir` + `output_stem` (serde default) |
| Modify: `src-tauri/src/download.rs` | probe BV→batch; `run_download` subdir/stem; batch uses item fields |
| Modify: `src-tauri/src/site.rs` | `is_bilibili_video_url` helper (optional, can live in bilibili.rs) |
| Modify: `src-tauri/src/library.rs` | recursive `list_videos` |
| Fixture: `src-tauri/tests/fixtures/bilibili_view_ugc_season.json` | trimmed view API sample |

---

### Task 1: Sanitize + extract BV (unit tests first)

**Files:**
- Create: `src-tauri/src/bilibili.rs`
- Modify: `src-tauri/src/lib.rs` (add `mod bilibili;`)

- [ ] **Step 1: Write failing tests in `bilibili.rs`**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_bvid_from_video_url() {
        assert_eq!(
            extract_bvid("https://www.bilibili.com/video/BV1NCgVzoEG9/?spm_id_from=333.788"),
            Some("BV1NCgVzoEG9".into())
        );
        assert_eq!(
            extract_bvid("https://www.bilibili.com/video/bv1ncgvzoeg9"),
            Some("BV1NCgVzoEG9".into()) // normalize: keep BV prefix + original alnum casing from capture; prefer uppercase BV + rest as in URL after BV
        );
        assert_eq!(extract_bvid("https://space.bilibili.com/1/lists/2"), None);
    }

    #[test]
    fn sanitize_path_component_strips_illegal() {
        assert_eq!(sanitize_path_component("A/B:C*"), "A_B_C_");
        assert_eq!(sanitize_path_component("  hi  "), "hi");
        assert!(!sanitize_path_component(&"x".repeat(200)).chars().count() > 120);
    }
}
```

Implementation note for `extract_bvid`: match `/video/(BV[0-9A-Za-z]+)/i`, return with `BV` uppercase and the rest as captured (or full uppercase — pick one and keep tests consistent). Prefer: find `BV`/`bv` + 10+ alnum, return `BV` + remainder as captured from URL without forcing case on body.

For sanitize: replace `/ \ : * ? " < > |` and control chars with `_`; trim; collapse empty to `"untitled"`; truncate to 80 chars.

- [ ] **Step 2: Run tests — expect FAIL (module/functions missing)**

```bash
cd src-tauri && cargo test bilibili::tests -- --nocapture
```

Expected: compile error or FAIL

- [ ] **Step 3: Implement minimal `extract_bvid` + `sanitize_path_component`**

```rust
// src-tauri/src/bilibili.rs
pub fn extract_bvid(url: &str) -> Option<String> {
    let lower = url.to_lowercase();
    let marker = "/video/";
    let idx = lower.find(marker)?;
    let rest = &url[idx + marker.len()..];
    let token = rest.split(['/', '?', '#']).next().unwrap_or("");
    if token.len() >= 3 && token[..2].eq_ignore_ascii_case("BV") {
        Some(format!("BV{}", &token[2..]))
    } else {
        None
    }
}

pub fn sanitize_path_component(name: &str) -> String {
    const MAX: usize = 80;
    let mut out = String::new();
    for c in name.chars() {
        if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control() {
            out.push('_');
        } else {
            out.push(c);
        }
    }
    let trimmed = out.trim().trim_matches('.');
    let s = if trimmed.is_empty() {
        "untitled".to_string()
    } else {
        trimmed.to_string()
    };
    if s.chars().count() <= MAX {
        s
    } else {
        s.chars().take(MAX).collect()
    }
}
```

Wire `mod bilibili;` in `lib.rs`.

- [ ] **Step 4: Re-run tests — expect PASS**

```bash
cd src-tauri && cargo test bilibili::tests -- --nocapture
```

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/bilibili.rs src-tauri/src/lib.rs
git commit -m "feat: add bilibili BV extract and path sanitize helpers"
```

---

### Task 2: Parse ugc_season from view JSON

**Files:**
- Create: `src-tauri/tests/fixtures/bilibili_view_ugc_season.json`
- Modify: `src-tauri/src/bilibili.rs`

- [ ] **Step 1: Add fixture** (trimmed shape; 2 episodes, 2+2 pages is enough)

```json
{
  "code": 0,
  "data": {
    "bvid": "BV1NCgVzoEG9",
    "title": "【闪客】一小时从函数到 Transformer",
    "ugc_season": {
      "id": 4808015,
      "title": "AI入门",
      "sections": [
        {
          "title": "正片",
          "episodes": [
            {
              "bvid": "BV1NCgVzoEG9",
              "title": "【完整合集】一小时从函数到Transformer！",
              "pages": [
                { "page": 1, "part": "01 从函数到神经网络" },
                { "page": 2, "part": "02 计算神经网络的参数" }
              ]
            },
            {
              "bvid": "BV15z4C6SEHT",
              "title": "【闪客】一小时从 Transformer 到大模型！",
              "pages": [
                { "page": 1, "part": "00 没什么用的前言" },
                { "page": 2, "part": "01 定义问题" }
              ]
            }
          ]
        }
      ]
    }
  }
}
```

- [ ] **Step 2: Write failing test**

```rust
#[test]
fn parse_ugc_season_expands_all_pages() {
    let raw = include_str!("../../tests/fixtures/bilibili_view_ugc_season.json");
    let items = parse_ugc_season_items(raw).unwrap();
    assert_eq!(items.len(), 4);
    assert_eq!(items[0].id, "BV1NCgVzoEG9_p1");
    assert_eq!(items[0].title, "01 从函数到神经网络");
    assert_eq!(
        items[0].url,
        "https://www.bilibili.com/video/BV1NCgVzoEG9?p=1"
    );
    assert_eq!(
        items[0].subdir,
        "AI入门/【完整合集】一小时从函数到Transformer！"
    ); // after sanitize — if sanitize leaves Chinese OK, path uses sanitized segments joined by /
    assert_eq!(items[0].output_stem, "01 从函数到神经网络 [BV1NCgVzoEG9_p1]");
    assert_eq!(items[2].id, "BV15z4C6SEHT_p1");
}

#[test]
fn parse_view_without_season_returns_none() {
    let raw = r#"{"code":0,"data":{"bvid":"BV1xx","title":"solo","pages":[{"page":1,"part":"p1"}]}}"#;
    assert!(parse_ugc_season_items(raw).unwrap().is_empty() || matches!(parse_ugc_season_items(raw), Ok(v) if v.is_empty()));
}
```

Define:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeasonPart {
    pub id: String,
    pub title: String,
    pub url: String,
    pub subdir: String,       // "{season}/{episode}" relative, sanitized
    pub output_stem: String,  // "{part} [{id}]" sanitized
    pub season_title: String,
}
```

`parse_ugc_season_items(json: &str) -> Result<Vec<SeasonPart>, String>`:
- Require `code == 0` (if present)
- If `data.ugc_season` missing → `Ok(vec![])`
- Else walk sections → episodes → pages (if pages empty, one part with page=1 and part=episode title)
- Build fields with `sanitize_path_component` on season title, episode title, part title

- [ ] **Step 3: Run test — FAIL**

```bash
cd src-tauri && cargo test parse_ugc_season -- --nocapture
```

- [ ] **Step 4: Implement parse with `serde_json::Value` (no heavy structs required)**

- [ ] **Step 5: Run test — PASS**

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/bilibili.rs src-tauri/tests/fixtures/bilibili_view_ugc_season.json
git commit -m "feat: parse bilibili ugc_season view JSON into download parts"
```

---

### Task 3: Fetch view API with ureq

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/src/bilibili.rs`

- [ ] **Step 1: Add dependency**

```toml
ureq = { version = "2", features = ["json"] }
```

- [ ] **Step 2: Implement fetch**

```rust
pub fn fetch_view_json(bvid: &str) -> Result<String, String> {
    let url = format!("https://api.bilibili.com/x/web-interface/view?bvid={bvid}");
    let body = ureq::get(&url)
        .set("User-Agent", "Mozilla/5.0 (compatible; VideoFetch/0.1)")
        .set("Referer", "https://www.bilibili.com/")
        .call()
        .map_err(|e| format!("请求 B 站视频信息失败: {e}"))?
        .into_string()
        .map_err(|e| format!("读取 B 站视频信息失败: {e}"))?;
    Ok(body)
}

/// Returns Some(parts) if ugc_season present and non-empty; None if no season.
pub fn try_expand_ugc_season_from_bv_url(page_url: &str) -> Result<Option<Vec<SeasonPart>>, String> {
    let Some(bvid) = extract_bvid(page_url) else {
        return Ok(None);
    };
    let json = fetch_view_json(&bvid)?;
    let items = parse_ugc_season_items(&json)?;
    if items.is_empty() {
        Ok(None)
    } else {
        Ok(Some(items))
    }
}
```

- [ ] **Step 3: Manual smoke (network)**

```bash
cd src-tauri && cargo test -- --ignored  # only if you add an #[ignore] live test
# or a tiny bin/example — optional. Prefer:
cargo test bilibili::tests -- --nocapture
```

Optional ignored live test:

```rust
#[test]
#[ignore]
fn live_fetch_ai_rumen_season() {
    let parts = try_expand_ugc_season_from_bv_url(
        "https://www.bilibili.com/video/BV1NCgVzoEG9/",
    )
    .unwrap()
    .expect("season");
    assert!(parts.len() >= 17);
}
```

Run: `cargo test live_fetch_ai_rumen_season -- --ignored --nocapture`  
Expected: PASS with len ≥ 17

- [ ] **Step 4: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/bilibili.rs
git commit -m "feat: fetch bilibili view API to expand ugc_season"
```

---

### Task 4: Extend PlaylistItem + QueueItem

**Files:**
- Modify: `src-tauri/src/playlist.rs`
- Modify: `src-tauri/src/queue.rs`

- [ ] **Step 1: Extend structs**

```rust
// playlist.rs
#[derive(Debug, Clone, Serialize)]
pub struct PlaylistItem {
    pub id: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subdir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_stem: Option<String>,
}
```

Update `parse_flat_playlist_output` to set `url/subdir/output_stem: None`.

```rust
// queue.rs — QueueItem
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueItem {
    pub index: usize,
    pub id: String,
    pub title: String,
    pub url: String,
    pub status: ItemStatus,
    #[serde(default)]
    pub subdir: Option<String>,
    #[serde(default)]
    pub output_stem: Option<String>,
}
```

Fix all `QueueItem { ... }` construction sites to include `subdir: None, output_stem: None` (or rely on .. if using Default — prefer explicit/`None`).

- [ ] **Step 2: Add helper**

```rust
// playlist.rs
pub fn resolve_item_url(page_url: &str, item: &PlaylistItem) -> String {
    if let Some(ref u) = item.url {
        if !u.is_empty() {
            return u.clone();
        }
    }
    item_video_url(page_url, &item.id)
}
```

Update existing playlist unit tests to still compile (add `None` fields in any manual structs).

- [ ] **Step 3: `cargo test` — PASS**

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/playlist.rs src-tauri/src/queue.rs
git commit -m "feat: carry per-item url/subdir/output_stem for batch downloads"
```

---

### Task 5: `run_download` supports subdir + fixed stem

**Files:**
- Modify: `src-tauri/src/download.rs` (`run_download`, `run_download_once`)

- [ ] **Step 1: Extend signatures**

Add params (after `category`):

```rust
subdir: Option<&str>,
output_stem: Option<&str>,
```

Logic in `run_download_once`:

```rust
let mut out_dir = root.join(category);
if let Some(sub) = subdir {
    // sub is "Season/Episode" with sanitized components only — join carefully
    for part in sub.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            continue;
        }
        out_dir = out_dir.join(part);
    }
}
std::fs::create_dir_all(&out_dir).map_err(|e| format!("创建合集目录失败: {e}"))?;

let template = if let Some(stem) = output_stem {
    out_dir
        .join(format!("{stem}.%(ext)s"))
        .to_string_lossy()
        .into_owned()
} else {
    out_dir
        .join("%(title)s [%(id)s].%(ext)s")
        .to_string_lossy()
        .into_owned()
};
```

Still call `library::create_category(&root, category)` for the top-level category only (unchanged). Do **not** pass nested path into `create_category`.

Update all call sites of `run_download` / `run_download_once` to pass `None, None` except batch workers (next task).

- [ ] **Step 2: Compile**

```bash
cd src-tauri && cargo test --lib 2>&1 | tail -30
```

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/download.rs
git commit -m "feat: allow nested out dir and fixed filename stem per download"
```

---

### Task 6: Wire BV → ugc_season batch in start + workers

**Files:**
- Modify: `src-tauri/src/download.rs` (`start_download`, `run_batch_download`)

- [ ] **Step 1: Probe in `start_download`**

Replace kind detection roughly with:

```rust
let mut kind = if site::is_batch_url(&url) {
    queue::QueueKind::Batch
} else {
    queue::QueueKind::Single
};
let mut prefetched_season: Option<Vec<crate::bilibili::SeasonPart>> = None;
if kind == queue::QueueKind::Single && crate::bilibili::extract_bvid(&url).is_some() {
    match crate::bilibili::try_expand_ugc_season_from_bv_url(&url) {
        Ok(Some(parts)) => {
            kind = queue::QueueKind::Batch;
            prefetched_season = Some(parts);
        }
        Ok(None) => {}
        Err(e) => {
            // Fail fast per spec verification #5 — do not silently download P1 only
            return Err(e);
        }
    }
}
```

Problem: `prefetched_season` must reach `run_batch_download`. Options:
- Store serialized parts on the job (heavy), or
- Re-fetch inside `run_batch_download` when `extract_bvid(page_url).is_some()` and not space-lists URL.

**Prefer re-fetch inside `run_batch_download`** (simpler, no job schema change):

```rust
// start_download kind only:
if kind == Single && extract_bvid(&url).is_some() {
    match try_expand_ugc_season_from_bv_url(&url) {
        Ok(Some(_)) => kind = Batch,
        Ok(None) => {}
        Err(e) => return Err(e),
    }
}
```

Double fetch is OK (once at start for kind, once in batch for items). To avoid double fetch, cache is optional YAGNI — **accept double fetch** unless easy: store `Option<Vec<SeasonPart>>` in thread via expanding only in batch and setting kind=Batch when bilibili video URL **optimistically**, then if expand returns None fall back to single… Spec wants: no season → single. Optimistic Batch then empty is wrong.

**Cleaner single fetch:** in `spawn_job_thread`, before match on kind:

Actually simplest UX-correct approach:

```rust
// start_download: always Single unless is_batch_url OR probe says season
```

Probe once; if season, `kind=Batch` and stash parts in a process-local map `SEASON_PREFETCH: Mutex<HashMap<job_id, Vec<SeasonPart>>>` cleared after batch start. A bit ugly.

**Plan choice:** re-fetch in `run_batch_download` only; in `start_download` set Batch if `extract_bvid` and a **lightweight** check — same full fetch once only in `start_download`, pass parts via:

```rust
// Add to DownloadJob? Too big.
```

Use thread-local / map keyed by job_id:

```rust
static PREFETCH: LazyLock<Mutex<HashMap<String, Vec<SeasonPart>>>> = ...;
// start_download after probe Ok(Some(parts)): PREFETCH.insert(job_id, parts); kind=Batch;
// run_batch_download: if let Some(parts) = PREFETCH.lock().remove(job_id) { use } else { expand_playlist ... }
```

Document this in code comment.

- [ ] **Step 2: In `run_batch_download` fresh expand branch**

```rust
} else {
    emit_line(app, job_id, "正在解析合集…");
    let items: Vec<PlaylistItem> = if let Some(parts) = take_prefetch(job_id) {
        parts
            .into_iter()
            .map(|p| PlaylistItem {
                id: p.id,
                title: p.title,
                url: Some(p.url),
                subdir: Some(p.subdir),
                output_stem: Some(p.output_stem),
            })
            .collect()
    } else if crate::bilibili::extract_bvid(page_url).is_some() {
        let parts = crate::bilibili::try_expand_ugc_season_from_bv_url(page_url)?
            .ok_or_else(|| "合集为空或无法解析条目".to_string())?;
        parts.into_iter().map(|p| PlaylistItem { ... }).collect()
    } else {
        crate::playlist::expand_playlist(page_url, &settings, job_id)?
    };
    // queue_items: use resolve_item_url + subdir/output_stem
    ...
}
```

Resume branch: rebuild `PlaylistItem` from `QueueItem` including `url: Some(i.url.clone())`, `subdir`, `output_stem`.

- [ ] **Step 3: Worker uses item fields**

```rust
let url = crate::playlist::resolve_item_url(&page_url, item);
match run_download(
    &app, &job_id, &url, &category,
    item.subdir.as_deref(),
    item.output_stem.as_deref(),
    &quality, audio_only, false, false,
) { ... }
```

(Adjust arg order to match Task 5 signature.)

- [ ] **Step 4: Job title** when season: use first item’s season folder name or `"AI入门 等 N 个"` — from prefetch `parts[0].season_title`.

Add `season_title` already on `SeasonPart`; when mapping, set job title to `format!("{} · {} 集", season_title, items.len())`.

- [ ] **Step 5: Unit/compile tests**

```bash
cd src-tauri && cargo test --lib
```

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/download.rs
git commit -m "feat: download full bilibili ugc_season from BV video URLs"
```

---

### Task 7: Recursive library listing

**Files:**
- Modify: `src-tauri/src/library.rs`

Without this, nested downloads are invisible in the library UI.

- [ ] **Step 1: Write failing test** (tempdir)

```rust
#[test]
fn list_videos_recurses_into_subdirs() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let nested = root.join("未分类").join("AI入门").join("课1");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(nested.join("a [BV1_p1].mp4"), b"x").unwrap();
    std::fs::write(root.join("未分类").join("top.mp4"), b"y").unwrap();
    let items = list_videos(root, "未分类").unwrap();
    assert_eq!(items.len(), 2);
    assert!(items.iter().any(|v| v.name == "a [BV1_p1].mp4"));
    assert!(items.iter().any(|v| v.name == "top.mp4"));
}
```

- [ ] **Step 2: Implement DFS/BFS in `list_videos`** — collect files under category; `name` can stay file name only (existing UI); `path` absolute/full string as today. Skip `.part` / fragments via existing `is_video`.

- [ ] **Step 3: Check `move_video` / delete** — if they assume flat filename only, leave as-is (user moves by filename from list); nested move may need full relative path later — **out of scope** unless broken. If `move_video` joins `category/filename` only, nested files cannot move — acceptable for本期; document in commit body.

- [ ] **Step 4: `cargo test list_videos_recurses` — PASS**

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/library.rs
git commit -m "feat: list library videos recursively under category"
```

---

### Task 8: Mark spec approved + verify live expand

**Files:**
- Modify: `docs/superpowers/specs/2026-09-24-bilibili-ugc-season-from-bv-design.md` — status → `已批准`

- [ ] **Step 1: Run ignored live test**

```bash
cd src-tauri && cargo test live_fetch_ai_rumen_season -- --ignored --nocapture
```

Expected: ≥ 17 parts; subdirs start with sanitized `AI入门/`.

- [ ] **Step 2: Manual app check (if binary available)**

Paste `https://www.bilibili.com/video/BV1NCgVzoEG9/` → batch table ~17 rows; stop after 1–2 completes; confirm paths under `{category}/AI入门/...`.

- [ ] **Step 3: Commit status + any test fixes**

```bash
git add docs/superpowers/specs/2026-09-24-bilibili-ugc-season-from-bv-design.md
git commit -m "docs: mark ugc_season-from-BV design approved"
```

---

## Spec coverage checklist

| Spec requirement | Task |
|---|---|
| BV + ugc_season → full season | 2, 3, 6 |
| Nested `{season}/{episode}/` | 2, 5, 6 |
| Filename `[bvid_pN]` | 2, 5 |
| No season → single unchanged | 6 |
| Space lists unchanged | 6 (else branch) |
| Batch UI/events reuse | 6 |
| API fail → error not silent P1 | 6 |
| Library sees nested files | 7 |
| Stop / concurrency | existing batch (unchanged) |

## Placeholder scan

None intentional; live UI download is manual in Task 8.

## Type consistency

- `SeasonPart` → mapped to `PlaylistItem { id, title, url, subdir, output_stem }`
- `QueueItem` mirrors `subdir` / `output_stem` for resume
- `resolve_item_url` used in workers and queue construction

---

## Execution handoff

Plan complete and saved to `docs/superpowers/plans/2026-09-24-bilibili-ugc-season-from-bv.md`.

**Two execution options:**

1. **Subagent-Driven (recommended)** — fresh subagent per task, review between tasks  
2. **Inline Execution** — execute in this session with executing-plans checkpoints  

Which approach?
