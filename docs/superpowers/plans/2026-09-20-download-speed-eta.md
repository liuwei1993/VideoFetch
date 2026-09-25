# Download Speed & ETA Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show download speed and estimated time remaining next to the progress percentage while a download is running.

**Architecture:** Parse `speed` and `eta` from yt-dlp progress lines in Rust (alongside existing `parse_percent`), emit them on `download-progress`, and render them via Ant Design `Progress` `format` in `DownloadView`. Frontend keeps the last non-null speed/eta until cleared on start/finish/error.

**Tech Stack:** Tauri (Rust) + React + Ant Design + yt-dlp progress text

**Spec:** `docs/superpowers/specs/2026-09-20-download-speed-eta-design.md`

---

### File map

| File | Responsibility |
|------|----------------|
| `src-tauri/src/download.rs` | Parse speed/ETA; extend `DownloadProgress`; emit fields |
| `src/types.ts` | TS type for new fields |
| `src/views/DownloadView.tsx` | State + Progress `format` display |

---

### Task 1: Rust parsers + unit tests (TDD)

**Files:**
- Modify: `src-tauri/src/download.rs`

- [ ] **Step 1: Add failing tests** for speed/ETA parsing

In the existing `#[cfg(test)] mod tests` block in `src-tauri/src/download.rs`, add:

```rust
#[test]
fn parse_download_speed_and_eta() {
    let line = "[download]  45.2% of  237.23MiB at    7.54MiB/s ETA 00:25";
    assert_eq!(parse_speed(line).as_deref(), Some("7.5 MB/s"));
    assert_eq!(parse_eta(line).as_deref(), Some("0:25"));
}

#[test]
fn parse_eta_with_hours() {
    let line = "[download]  10.0% of 1.00GiB at 1.20MiB/s ETA 01:02:03";
    assert_eq!(parse_eta(line).as_deref(), Some("1:02:03"));
    assert_eq!(parse_speed(line).as_deref(), Some("1.2 MB/s"));
}

#[test]
fn parse_speed_eta_absent_or_unknown() {
    assert_eq!(parse_speed("hello"), None);
    assert_eq!(parse_eta("hello"), None);
    assert_eq!(
        parse_eta("[download]  1.0% of 10.00MiB at 1.00MiB/s ETA Unknown"),
        None
    );
    assert_eq!(
        parse_eta("[download]  1.0% of 10.00MiB at 1.00MiB/s ETA --:--"),
        None
    );
    assert_eq!(parse_speed("写入中 12.0 MB · foo.part"), None);
}
```

- [ ] **Step 2: Run tests — expect FAIL** (functions missing)

```bash
cd src-tauri && cargo test parse_download_speed_and_eta parse_eta_with_hours parse_speed_eta_absent_or_unknown -- --nocapture
```

Expected: compile error — `parse_speed` / `parse_eta` not found.

- [ ] **Step 3: Implement `parse_speed` and `parse_eta`**

Add after `parse_percent` in `src-tauri/src/download.rs`:

```rust
fn split_speed_value(raw: &str) -> Option<(f64, String)> {
    let end = raw
        .char_indices()
        .find(|(_, c)| c.is_ascii_alphabetic())
        .map(|(i, _)| i)?;
    let (num_str, unit) = raw.split_at(end);
    let num: f64 = num_str.parse().ok()?;
    Some((num, unit.to_ascii_lowercase()))
}

fn parse_speed(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if !trimmed.contains("[download]") {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();
    let at_idx = lower.find(" at ")?;
    let after_at = trimmed[at_idx + 4..].trim_start();
    let slash_s = after_at.to_ascii_lowercase().find("/s")?;
    let raw = after_at[..slash_s].trim();
    let (num, unit) = split_speed_value(raw)?;
    let label = match unit.as_str() {
        "kib" | "kb" => "KB/s",
        "mib" | "mb" => "MB/s",
        "gib" | "gb" => "GB/s",
        "b" => "B/s",
        _ => return None,
    };
    Some(format!("{num:.1} {label}"))
}

fn parse_eta(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if !trimmed.contains("[download]") {
        return None;
    }
    let upper = trimmed.to_ascii_uppercase();
    let eta_idx = upper.find("ETA ")?;
    let rest = trimmed[eta_idx + 4..].trim_start();
    let token = rest.split_whitespace().next()?.trim();
    let lower = token.to_ascii_lowercase();
    if lower == "unknown" || token.contains('-') {
        return None;
    }
    let parts: Vec<&str> = token.split(':').collect();
    match parts.as_slice() {
        [mm, ss] => {
            let m: u32 = mm.parse().ok()?;
            let s: u32 = ss.parse().ok()?;
            Some(format!("{m}:{s:02}"))
        }
        [hh, mm, ss] => {
            let h: u32 = hh.parse().ok()?;
            let m: u32 = mm.parse().ok()?;
            let s: u32 = ss.parse().ok()?;
            if h == 0 {
                Some(format!("{m}:{s:02}"))
            } else {
                Some(format!("{h}:{m:02}:{s:02}"))
            }
        }
        _ => None,
    }
}
```

Note: `00:25` → `0:25` because minutes are not zero-padded when hours are absent or zero.

- [ ] **Step 4: Run tests — expect PASS**

```bash
cd src-tauri && cargo test parse_download_ -- --nocapture
```

Expected: all parse tests PASS (including existing `parse_download_percent`).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/download.rs
git commit -m "$(cat <<'EOF'
feat: parse yt-dlp download speed and ETA from progress lines

EOF
)"
```

---

### Task 2: Extend `DownloadProgress` emit

**Files:**
- Modify: `src-tauri/src/download.rs`

- [ ] **Step 1: Extend struct**

```rust
#[derive(Clone, Serialize)]
pub struct DownloadProgress {
    pub percent: Option<f64>,
    pub line: String,
    pub speed: Option<String>,
    pub eta: Option<String>,
}
```

- [ ] **Step 2: Update every `DownloadProgress { ... }` construction**

In `emit_line`:

```rust
fn emit_line(app: &AppHandle, line: &str) {
    let line = line.trim();
    if line.is_empty() {
        return;
    }
    let percent = parse_percent(line);
    let speed = parse_speed(line);
    let eta = parse_eta(line);
    let _ = app.emit(
        "download-progress",
        DownloadProgress {
            percent,
            line: line.to_string(),
            speed,
            eta,
        },
    );
}
```

For non-progress emits (`watch_part_files`, startup log, stop message), set `speed: None, eta: None`.

- [ ] **Step 3: Compile check**

```bash
cd src-tauri && cargo test parse_download_ && cargo check
```

Expected: PASS / OK.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/download.rs
git commit -m "$(cat <<'EOF'
feat: emit speed and ETA on download-progress events

EOF
)"
```

---

### Task 3: Frontend types + Progress display

**Files:**
- Modify: `src/types.ts`
- Modify: `src/views/DownloadView.tsx`

- [ ] **Step 1: Extend TS type**

```ts
export type DownloadProgress = {
  percent: number | null;
  line: string;
  speed: string | null;
  eta: string | null;
};
```

- [ ] **Step 2: State + listener updates in `DownloadView.tsx`**

Add state:

```ts
const [speed, setSpeed] = useState<string | null>(null);
const [eta, setEta] = useState<string | null>(null);
```

In `download-progress` listener:

```ts
if (e.payload.percent != null) setPercent(e.payload.percent);
if (e.payload.speed != null) setSpeed(e.payload.speed);
if (e.payload.eta != null) setEta(e.payload.eta);
```

In `onStart`:

```ts
setPercent(0);
setSpeed(null);
setEta(null);
```

In `download-finished` and `download-error` handlers:

```ts
setSpeed(null);
setEta(null);
```

(Keep `setPercent(100)` on finished as today.)

- [ ] **Step 3: Progress `format`**

Replace the `Progress` block with:

```tsx
{percent != null && (
  <Progress
    percent={Math.min(Number(percent.toFixed(1)), 100)}
    status={busy ? "active" : percent >= 100 ? "success" : "normal"}
    format={(p) => {
      const parts = [`${p}%`];
      if (busy && speed) parts.push(speed);
      if (busy && eta) parts.push(`剩余 ${eta}`);
      return parts.join(" · ");
    }}
    style={{ marginBottom: 16 }}
  />
)}
```

- [ ] **Step 4: Typecheck**

```bash
npx tsc --noEmit
```

Expected: no errors related to `DownloadProgress` / `DownloadView`.

- [ ] **Step 5: Commit**

```bash
git add src/types.ts src/views/DownloadView.tsx
git commit -m "$(cat <<'EOF'
feat: show download speed and ETA beside progress percent

EOF
)"
```

---

### Task 4: Manual verification

- [ ] Start `npm run tauri dev` (or reuse running app)
- [ ] Download a short video; confirm progress text like `45% · 7.5 MB/s · 剩余 0:25`
- [ ] During merge/transcode (no speed lines), last speed/ETA remains until finish
- [ ] On complete: shows success / `100%` without stale speed/ETA
- [ ] On stop: speed/ETA cleared

---

## Spec coverage check

| Spec item | Task |
|-----------|------|
| UI beside percent | Task 3 |
| Format strings | Task 3 |
| Retain last speed/ETA | Task 3 |
| Clear on start/finish/error | Task 3 |
| Parse + emit fields | Tasks 1–2 |
| Unit tests | Task 1 |
| Non-goals (no part-file ETA, etc.) | Not implemented (intentional) |
