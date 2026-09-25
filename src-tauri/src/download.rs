use crate::library::{self, UNCATEGORIZED};
use crate::queue;
use crate::settings::{self};
use crate::site::{self, Site};
use serde::Deserialize;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, LazyLock, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    pub job_id: String,
    pub percent: Option<f64>,
    pub line: String,
    pub speed: Option<String>,
    pub eta: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadFinished {
    pub job_id: String,
    pub path: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadError {
    pub job_id: String,
    pub message: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchItemMeta {
    pub id: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadBatchStarted {
    pub job_id: String,
    pub total: usize,
    pub items: Vec<BatchItemMeta>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadItemStarted {
    pub job_id: String,
    pub index: usize,
    pub id: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadItemFinished {
    pub job_id: String,
    pub index: usize,
    pub id: String,
    pub path: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadItemError {
    pub job_id: String,
    pub index: usize,
    pub id: String,
    pub message: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadBatchFinished {
    pub job_id: String,
    pub succeeded: usize,
    pub failed: usize,
    pub cancelled: usize,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartDownloadResult {
    pub job_id: String,
}

enum SessionOutcome {
    Single(PathBuf),
    Batch,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartDownloadArgs {
    pub url: String,
    pub category: String,
    pub quality: String,
    #[serde(default)]
    pub audio_only: bool,
}

struct JobRuntime {
    cancelled: AtomicBool,
    pids: Mutex<HashSet<u32>>,
}

impl JobRuntime {
    fn new() -> Self {
        Self {
            cancelled: AtomicBool::new(false),
            pids: Mutex::new(HashSet::new()),
        }
    }
}

struct SlotPool {
    in_use: Mutex<usize>,
    cvar: Condvar,
    cap: AtomicUsize,
}

impl SlotPool {
    fn new() -> Self {
        Self {
            in_use: Mutex::new(0),
            cvar: Condvar::new(),
            cap: AtomicUsize::new(5),
        }
    }

    fn set_capacity(&self, n: usize) {
        self.cap.store(n.max(1), Ordering::SeqCst);
        self.cvar.notify_all();
    }

    fn acquire(&self, cancelled: &AtomicBool) -> Result<SlotGuard<'_>, String> {
        let mut in_use = self.in_use.lock().map_err(|e| e.to_string())?;
        loop {
            if cancelled.load(Ordering::SeqCst) {
                return Err("已停止下载".into());
            }
            let cap = self.cap.load(Ordering::SeqCst).max(1);
            if *in_use < cap {
                *in_use += 1;
                return Ok(SlotGuard {
                    pool: self,
                    released: false,
                });
            }
            let (guard, _) = self
                .cvar
                .wait_timeout(in_use, Duration::from_millis(200))
                .map_err(|e| e.to_string())?;
            in_use = guard;
        }
    }

    fn release(&self) {
        if let Ok(mut in_use) = self.in_use.lock() {
            *in_use = in_use.saturating_sub(1);
        }
        self.cvar.notify_all();
    }

    fn notify_waiters(&self) {
        self.cvar.notify_all();
    }
}

pub fn set_slot_capacity(n: u32) {
    SLOT_POOL.set_capacity(settings::clamp_max_concurrent(n) as usize);
}

struct SlotGuard<'a> {
    pool: &'a SlotPool,
    released: bool,
}

impl Drop for SlotGuard<'_> {
    fn drop(&mut self) {
        if !self.released {
            self.pool.release();
            self.released = true;
        }
    }
}

static SLOT_POOL: LazyLock<SlotPool> = LazyLock::new(SlotPool::new);
static RESOURCE_DIR: LazyLock<Mutex<Option<PathBuf>>> = LazyLock::new(|| Mutex::new(None));
static JOB_SEQ: AtomicU64 = AtomicU64::new(1);
static JOBS: LazyLock<Mutex<HashMap<String, queue::DownloadJob>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static RUNTIMES: LazyLock<Mutex<HashMap<String, Arc<JobRuntime>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static HYDRATED: AtomicBool = AtomicBool::new(false);
static QUEUE_PERSIST_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));
static SEASON_PREFETCH: LazyLock<Mutex<HashMap<String, Vec<crate::bilibili::SeasonPart>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn next_job_id() -> String {
    let n = JOB_SEQ.fetch_add(1, Ordering::SeqCst);
    format!("job-{n}")
}

fn bump_seq_from_id(id: &str) {
    if let Some(n) = id.strip_prefix("job-").and_then(|s| s.parse::<u64>().ok()) {
        JOB_SEQ
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |cur| {
                if n >= cur {
                    Some(n + 1)
                } else {
                    None
                }
            })
            .ok();
    }
}

fn runtime_for(job_id: &str) -> Option<Arc<JobRuntime>> {
    RUNTIMES.lock().ok()?.get(job_id).cloned()
}

fn install_runtime(job_id: &str) -> Arc<JobRuntime> {
    let rt = Arc::new(JobRuntime::new());
    if let Ok(mut g) = RUNTIMES.lock() {
        g.insert(job_id.to_string(), rt.clone());
    }
    rt
}

fn remove_runtime(job_id: &str) {
    if let Ok(mut g) = RUNTIMES.lock() {
        g.remove(job_id);
    }
}

pub(crate) fn add_child_pid(job_id: &str, pid: u32) {
    if let Some(rt) = runtime_for(job_id) {
        if let Ok(mut g) = rt.pids.lock() {
            g.insert(pid);
        }
    }
}

pub(crate) fn remove_child_pid(job_id: &str, pid: u32) {
    if let Some(rt) = runtime_for(job_id) {
        if let Ok(mut g) = rt.pids.lock() {
            g.remove(&pid);
        }
    }
}

pub(crate) fn is_job_cancelled(job_id: &str) -> bool {
    runtime_for(job_id)
        .map(|rt| rt.cancelled.load(Ordering::SeqCst))
        .unwrap_or(false)
}

fn cancel_job_runtime(job_id: &str) -> Vec<u32> {
    let Some(rt) = runtime_for(job_id) else {
        return Vec::new();
    };
    rt.cancelled.store(true, Ordering::SeqCst);
    SLOT_POOL.notify_waiters();
    rt.pids
        .lock()
        .ok()
        .map(|g| g.iter().copied().collect())
        .unwrap_or_default()
}

fn hydrate_jobs(app: &AppHandle) {
    if HYDRATED.load(Ordering::SeqCst) {
        return;
    }
    let Ok(store) = queue::load_job_store(app) else {
        return;
    };
    let Ok(mut jobs) = JOBS.lock() else {
        return;
    };
    if HYDRATED.load(Ordering::SeqCst) {
        return;
    }
    for mut job in store.jobs {
        bump_seq_from_id(&job.id);
        if job.status == queue::JobStatus::Downloading {
            job.status = queue::JobStatus::Pending;
        }
        jobs.insert(job.id.clone(), job);
    }
    HYDRATED.store(true, Ordering::SeqCst);
}

fn job_num(id: &str) -> u64 {
    id.strip_prefix("job-")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

fn snapshot_jobs() -> Vec<queue::DownloadJob> {
    JOBS.lock()
        .ok()
        .map(|g| {
            let mut v: Vec<_> = g.values().cloned().collect();
            v.sort_by(|a, b| job_num(&b.id).cmp(&job_num(&a.id)));
            v
        })
        .unwrap_or_default()
}

fn persist_jobs(app: &AppHandle) {
    let _guard = QUEUE_PERSIST_LOCK.lock().ok();
    let jobs = snapshot_jobs();
    let _ = queue::save_incomplete_jobs(app, &jobs);
}

fn get_job(job_id: &str) -> Option<queue::DownloadJob> {
    JOBS.lock().ok()?.get(job_id).cloned()
}

fn upsert_job(app: &AppHandle, job: queue::DownloadJob, emit: bool) {
    let id = job.id.clone();
    if let Ok(mut jobs) = JOBS.lock() {
        jobs.insert(id.clone(), job);
    }
    persist_jobs(app);
    if emit {
        emit_job_upsert(app, &id);
    }
}

fn patch_job(app: &AppHandle, job_id: &str, emit: bool, f: impl FnOnce(&mut queue::DownloadJob)) {
    let mut changed = false;
    if let Ok(mut jobs) = JOBS.lock() {
        if let Some(job) = jobs.get_mut(job_id) {
            f(job);
            job.touch();
            changed = true;
        }
    }
    if changed {
        persist_jobs(app);
        if emit {
            emit_job_upsert(app, job_id);
        }
    }
}

fn emit_job_upsert(app: &AppHandle, job_id: &str) {
    if let Some(job) = get_job(job_id) {
        let _ = app.emit("download-job-upsert", job);
    }
}

fn job_overall_percent(job_id: &str, item_percent: Option<f64>) -> Option<f64> {
    let job = get_job(job_id)?;
    if job.kind == queue::QueueKind::Batch && job.total > 1 {
        match item_percent {
            Some(p) => Some(batch_overall_percent(job.completed, job.total, Some(p))),
            None => job.percent,
        }
    } else {
        item_percent.or(job.percent)
    }
}

fn batch_overall_percent(done: usize, total: usize, current: Option<f64>) -> f64 {
    if total == 0 {
        return 0.0;
    }
    let cur = current.unwrap_or(0.0).clamp(0.0, 100.0) / 100.0;
    ((done as f64) + cur) / (total as f64) * 100.0
}

fn item_status_label(status: queue::ItemStatus) -> String {
    match status {
        queue::ItemStatus::Pending => "pending",
        queue::ItemStatus::Downloading => "downloading",
        queue::ItemStatus::Done => "done",
        queue::ItemStatus::Failed => "failed",
    }
    .to_string()
}

fn persist_item_status(app: &AppHandle, job_id: &str, index: usize, status: queue::ItemStatus) {
    patch_job(app, job_id, false, |job| {
        job.set_item_status(index, status);
    });
}

fn emit_session_outcome(app: &AppHandle, job_id: &str, session: Result<SessionOutcome, String>) {
    if is_job_cancelled(job_id) {
        patch_job(app, job_id, true, |job| {
            job.status = queue::JobStatus::Cancelled;
            job.speed = None;
            job.eta = None;
            if job.detail.is_empty() {
                job.detail = "已停止下载".into();
            }
        });
        let _ = app.emit(
            "download-error",
            DownloadError {
                job_id: job_id.to_string(),
                message: "已停止下载".into(),
            },
        );
        return;
    }
    match session {
        Ok(SessionOutcome::Batch) => {}
        Ok(SessionOutcome::Single(path)) => {
            let path = path.to_string_lossy().into_owned();
            patch_job(app, job_id, true, |job| {
                if job.status == queue::JobStatus::Cancelled {
                    return;
                }
                job.status = queue::JobStatus::Done;
                job.percent = Some(100.0);
                job.path = Some(path.clone());
                job.speed = None;
                job.eta = None;
                job.detail = "已保存".into();
            });
            if get_job(job_id).map(|j| j.status) == Some(queue::JobStatus::Done) {
                let _ = app.emit(
                    "download-finished",
                    DownloadFinished {
                        job_id: job_id.to_string(),
                        path,
                    },
                );
            }
        }
        Err(message) => {
            let cancelled = message.contains("已停止");
            patch_job(app, job_id, true, |job| {
                if job.status == queue::JobStatus::Cancelled {
                    return;
                }
                job.status = if cancelled {
                    queue::JobStatus::Cancelled
                } else {
                    queue::JobStatus::Failed
                };
                job.error = if cancelled { None } else { Some(message.clone()) };
                job.speed = None;
                job.eta = None;
                job.detail = message.clone();
            });
            let _ = app.emit(
                "download-error",
                DownloadError {
                    job_id: job_id.to_string(),
                    message,
                },
            );
        }
    }
}

fn kill_pid(pid: u32) {
    // Kill the process group first (covers ffmpeg children), then the pid.
    let _ = Command::new("kill")
        .args(["-TERM", "--", &format!("-{pid}")])
        .status();
    let _ = Command::new("kill")
        .args(["-TERM", "--", &pid.to_string()])
        .status();
    // Escalate if still alive shortly after.
    std::thread::sleep(Duration::from_millis(300));
    let _ = Command::new("kill")
        .args(["-KILL", "--", &format!("-{pid}")])
        .status();
    let _ = Command::new("kill")
        .args(["-KILL", "--", &pid.to_string()])
        .status();
}

pub fn stop_download(app: AppHandle, job_id: String) -> Result<(), String> {
    hydrate_jobs(&app);
    let job_id = job_id.trim();
    if job_id.is_empty() {
        return Err("任务 ID 不能为空".into());
    }
    let Some(job) = get_job(job_id) else {
        return Err("找不到该下载任务".into());
    };
    if !job.status.is_active() && runtime_for(job_id).is_none() {
        return Err("该任务未在进行".into());
    }
    let pids = cancel_job_runtime(job_id);
    patch_job(&app, job_id, true, |job| {
        job.status = queue::JobStatus::Cancelled;
        job.detail = "已停止下载".into();
        job.speed = None;
        job.eta = None;
    });
    std::thread::spawn(move || {
        for pid in pids {
            kill_pid(pid);
        }
    });
    Ok(())
}

pub fn stop_all_downloads(app: AppHandle) -> Result<(), String> {
    hydrate_jobs(&app);
    let ids: Vec<String> = snapshot_jobs()
        .into_iter()
        .filter(|j| j.status.is_active())
        .map(|j| j.id)
        .collect();
    if ids.is_empty() {
        return Err("当前没有下载任务".into());
    }
    for id in ids {
        let _ = stop_download(app.clone(), id);
    }
    Ok(())
}

pub fn list_download_jobs(app: AppHandle) -> Result<Vec<queue::DownloadJob>, String> {
    hydrate_jobs(&app);
    Ok(snapshot_jobs())
}

pub fn init_resource_dir(app: &AppHandle) {
    if let Ok(dir) = app.path().resource_dir() {
        *RESOURCE_DIR.lock().expect("resource dir lock") = Some(dir);
    }
}

pub fn resolve_ytdlp() -> Result<(String, Vec<String>), String> {
    if let Ok(custom) = std::env::var("VIDEOFETCH_YTDLP") {
        if !custom.trim().is_empty() {
            return Ok((custom, vec![]));
        }
    }
    if let Some(bundled) = bundled_executable("yt-dlp") {
        return Ok((bundled.to_string_lossy().into_owned(), vec![]));
    }
    // `python -u -m yt_dlp` keeps progress unbuffered when stdout/stderr are pipes.
    if command_exists("uvx") {
        return Ok((
            "uvx".into(),
            vec![
                "--from".into(),
                "yt-dlp".into(),
                "python".into(),
                "-u".into(),
                "-m".into(),
                "yt_dlp".into(),
            ],
        ));
    }
    if command_exists("yt-dlp") {
        return Ok(("yt-dlp".into(), vec![]));
    }
    Err("未找到 yt-dlp。AppImage 应自带 yt-dlp；开发时请运行 scripts/fetch-linux-sidecars.sh，或安装 uv / yt-dlp，也可设置 VIDEOFETCH_YTDLP。".into())
}

/// Sidecar next to the app binary, in bundled resources, or the dev copy under src-tauri/binaries.
pub fn bundled_executable(name: &str) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join(name));
        }
    }
    if let Ok(guard) = RESOURCE_DIR.lock() {
        if let Some(res) = guard.as_ref() {
            candidates.push(res.join(name));
            candidates.push(res.join("binaries").join(name));
        }
    }
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("binaries");
    candidates.push(manifest.join(name));
    candidates.push(manifest.join(format!(
        "{name}-{}",
        env!("VIDEOFETCH_HOST_TRIPLE")
    )));
    candidates.into_iter().find(|p| p.is_file())
}

/// Point yt-dlp at the bundled ffmpeg directory when present.
pub fn apply_bundled_ffmpeg(cmd: &mut Command) {
    let Some(ffmpeg) = bundled_executable("ffmpeg") else {
        return;
    };
    let Some(dir) = ffmpeg.parent() else {
        return;
    };
    cmd.arg("--ffmpeg-location").arg(dir);
    let old = std::env::var_os("PATH").unwrap_or_default();
    let mut prefixed = dir.as_os_str().to_os_string();
    prefixed.push(":");
    prefixed.push(old);
    cmd.env("PATH", prefixed);
}

fn ffprobe_program() -> PathBuf {
    bundled_executable("ffprobe").unwrap_or_else(|| PathBuf::from("ffprobe"))
}

fn ytdlp_plugin_dirs(app: &AppHandle) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(res) = app.path().resource_dir() {
        let p = res.join("yt-dlp-plugins");
        if p.is_dir() {
            dirs.push(p);
        }
    }
    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("yt-dlp-plugins");
    if dev.is_dir() && !dirs.iter().any(|d| d == &dev) {
        dirs.push(dev);
    }
    dirs
}

#[cfg(test)]
fn missav_plugin_file() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("yt-dlp-plugins/missav/yt_dlp_plugins/extractor/missav.py")
}

const DOWNLOAD_ATTEMPTS: u32 = 3;
const YOUTUBE_MAX_CONCURRENT: usize = 2;

fn command_exists(name: &str) -> bool {
    Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {name}"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn node_available() -> bool {
    command_exists("node")
}

pub fn apply_ytdlp_retry_args(cmd: &mut Command) {
    cmd.arg("--retries")
        .arg("15")
        .arg("--fragment-retries")
        .arg("15")
        .arg("--extractor-retries")
        .arg("5")
        .arg("--retry-sleep")
        .arg("2")
        .arg("--socket-timeout")
        .arg("30");
}

fn youtube_batch_concurrency(configured: u32) -> usize {
    (settings::clamp_max_concurrent(configured) as usize)
        .min(YOUTUBE_MAX_CONCURRENT)
        .max(1)
}

fn is_ytdlp_diag_line(line: &str) -> bool {
    let t = line.trim();
    t.contains("ERROR:")
        || t.contains("HTTP Error")
        || t.contains("Unable to download")
        || t.contains("Connection refused")
        || t.contains("Errno 111")
        || t.contains("Giving up")
}

fn format_ytdlp_failure(code: Option<i32>, notes: &[String]) -> String {
    let hint = notes
        .iter()
        .rev()
        .find(|l| l.contains("ERROR:"))
        .or_else(|| notes.last())
        .cloned()
        .unwrap_or_default();
    if hint.is_empty() {
        format!("yt-dlp 退出码: {code:?}")
    } else {
        format!("yt-dlp 退出码: {code:?} · {hint}")
    }
}

fn parse_percent(line: &str) -> Option<f64> {
    // e.g. [download]  45.2% of ...
    let trimmed = line.trim();
    if !trimmed.contains("[download]") {
        return None;
    }
    let Some(idx) = trimmed.find('%') else {
        return None;
    };
    let before = &trimmed[..idx];
    let num: String = before
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    num.parse().ok()
}

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

fn emit_line(app: &AppHandle, job_id: &str, line: &str) {
    let line = line.trim();
    if line.is_empty() {
        return;
    }
    let item_percent = parse_percent(line);
    let speed = parse_speed(line);
    let eta = parse_eta(line);
    let percent = job_overall_percent(job_id, item_percent);
    if let Ok(mut jobs) = JOBS.lock() {
        if let Some(job) = jobs.get_mut(job_id) {
            if percent.is_some() {
                job.percent = percent;
            }
            if speed.is_some() {
                job.speed = speed.clone();
            }
            if eta.is_some() {
                job.eta = eta.clone();
            }
            job.detail = line.to_string();
        }
    }
    let _ = app.emit(
        "download-progress",
        DownloadProgress {
            job_id: job_id.to_string(),
            percent,
            line: line.to_string(),
            speed,
            eta,
        },
    );
}

/// Read pipe bytes and split on both `\n` and `\r` (yt-dlp progress often uses `\r`).
fn pump_pipe(
    app: AppHandle,
    job_id: String,
    mut pipe: impl Read,
    last_path: Arc<std::sync::Mutex<Option<String>>>,
    diag: Arc<std::sync::Mutex<Vec<String>>>,
) {
    let mut buf = [0u8; 4096];
    let mut acc = Vec::<u8>::new();
    loop {
        match pipe.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                for &b in &buf[..n] {
                    if b == b'\n' || b == b'\r' {
                        if !acc.is_empty() {
                            let line = String::from_utf8_lossy(&acc).into_owned();
                            if !line.starts_with('[') && Path::new(&line).is_absolute() {
                                if let Ok(mut g) = last_path.lock() {
                                    *g = Some(line.clone());
                                }
                            }
                            if is_ytdlp_diag_line(&line) {
                                if let Ok(mut g) = diag.lock() {
                                    g.push(line.trim().to_string());
                                    while g.len() > 8 {
                                        g.remove(0);
                                    }
                                }
                            }
                            emit_line(&app, &job_id, &line);
                            acc.clear();
                        }
                    } else {
                        acc.push(b);
                    }
                }
                let _ = std::io::stdout().flush();
            }
            Err(_) => break,
        }
    }
    if !acc.is_empty() {
        let line = String::from_utf8_lossy(&acc).into_owned();
        if !line.starts_with('[') && Path::new(&line).is_absolute() {
            if let Ok(mut g) = last_path.lock() {
                *g = Some(line.clone());
            }
        }
        if is_ytdlp_diag_line(&line) {
            if let Ok(mut g) = diag.lock() {
                g.push(line.trim().to_string());
            }
        }
        emit_line(&app, &job_id, &line);
    }
}

fn watch_part_files(app: AppHandle, job_id: String, out_dir: PathBuf, stop: Arc<AtomicBool>) {
    let mut last_bytes = 0u64;
    while !stop.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(800));
        let Ok(entries) = std::fs::read_dir(&out_dir) else {
            continue;
        };
        let mut total = 0u64;
        let mut name = String::new();
        for entry in entries.flatten() {
            let path = entry.path();
            let is_part = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case("part") || e.eq_ignore_ascii_case("ytdl"))
                .unwrap_or(false)
                || path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.contains(".part"))
                    .unwrap_or(false);
            if !is_part {
                continue;
            }
            if let Ok(meta) = entry.metadata() {
                total = total.max(meta.len());
                name = entry.file_name().to_string_lossy().into_owned();
            }
        }
        if total > 0 && total != last_bytes {
            last_bytes = total;
            let mb = total as f64 / 1024.0 / 1024.0;
            emit_line(&app, &job_id, &format!("写入中 {mb:.1} MB · {name}"));
        }
    }
}

fn spawn_job_thread(app: AppHandle, job: queue::DownloadJob, resume_items: Option<Vec<queue::QueueItem>>) {
    let job_id = job.id.clone();
    install_runtime(&job_id);
    std::thread::spawn(move || {
        let job_id = job.id.clone();
        let session = (|| -> Result<SessionOutcome, String> {
            if is_job_cancelled(&job_id) {
                return Err("已停止下载".into());
            }
            match job.kind {
                queue::QueueKind::Batch => {
                    run_batch_download(
                        &app,
                        &job_id,
                        &job.url,
                        &job.category,
                        &job.quality,
                        job.audio_only,
                        resume_items,
                    )?;
                    Ok(SessionOutcome::Batch)
                }
                queue::QueueKind::Single => {
                    let url = resume_items
                        .as_ref()
                        .and_then(|items| items.first())
                        .map(|i| i.url.clone())
                        .unwrap_or_else(|| job.url.clone());
                    match run_download(
                        &app,
                        &job_id,
                        &url,
                        &job.category,
                        None,
                        None,
                        &job.quality,
                        job.audio_only,
                        true,
                        true,
                    ) {
                        Ok(path) => Ok(SessionOutcome::Single(path)),
                        Err(message) => Err(message),
                    }
                }
            }
        })();
        if matches!(session, Ok(SessionOutcome::Batch)) {
            if is_job_cancelled(&job_id) {
                patch_job(&app, &job_id, true, |job| {
                    job.status = queue::JobStatus::Cancelled;
                    job.detail = "已停止下载".into();
                    job.speed = None;
                    job.eta = None;
                });
            } else {
                let job_now = get_job(&job_id);
                let failed = job_now
                    .as_ref()
                    .map(|j| {
                        j.items
                            .iter()
                            .any(|i| i.status == queue::ItemStatus::Failed)
                    })
                    .unwrap_or(false);
                patch_job(&app, &job_id, true, |job| {
                    if job.status == queue::JobStatus::Cancelled {
                        return;
                    }
                    job.status = if failed {
                        queue::JobStatus::Failed
                    } else {
                        queue::JobStatus::Done
                    };
                    job.percent = Some(100.0);
                    job.speed = None;
                    job.eta = None;
                    if job.detail.is_empty() {
                        job.detail = if failed {
                            "部分失败".into()
                        } else {
                            "已保存".into()
                        };
                    }
                });
            }
        } else {
            emit_session_outcome(&app, &job_id, session);
        }
        remove_runtime(&job_id);
    });
}

pub fn start_download(app: AppHandle, args: StartDownloadArgs) -> Result<StartDownloadResult, String> {
    hydrate_jobs(&app);
    let url = args.url.trim().to_string();
    if url.is_empty() {
        return Err("URL 不能为空".into());
    }

    let mut category = args.category.trim().to_string();
    if category.is_empty() {
        category = UNCATEGORIZED.to_string();
    }

    let quality = if args.quality.trim().is_empty() {
        "720".to_string()
    } else {
        args.quality.trim().to_string()
    };
    let audio_only = args.audio_only;
    let mut kind = if site::is_batch_url(&url) {
        queue::QueueKind::Batch
    } else {
        queue::QueueKind::Single
    };
    let mut season_parts: Option<Vec<crate::bilibili::SeasonPart>> = None;
    if kind == queue::QueueKind::Single && crate::bilibili::extract_bvid(&url).is_some() {
        match crate::bilibili::try_expand_ugc_season_from_bv_url(&url) {
            Ok(Some(parts)) => {
                kind = queue::QueueKind::Batch;
                season_parts = Some(parts);
            }
            Ok(None) => {}
            Err(e) => return Err(e),
        }
    }

    if let Ok(settings) = settings::load_settings(&app) {
        SLOT_POOL.set_capacity(settings::clamp_max_concurrent(settings.max_concurrent_downloads) as usize);
    }

    let job_id = next_job_id();
    if let Some(parts) = season_parts {
        if let Ok(mut map) = SEASON_PREFETCH.lock() {
            map.insert(job_id.clone(), parts);
        }
    }
    let mut job = queue::DownloadJob::new(
        job_id.clone(),
        url,
        category,
        quality,
        audio_only,
        kind,
    );
    if kind == queue::QueueKind::Single {
        let item_id = queue_item_id_from_url(&job.url);
        job.items = vec![queue::QueueItem {
            index: 0,
            id: item_id.clone(),
            title: item_id,
            url: job.url.clone(),
            status: queue::ItemStatus::Pending,
            subdir: None,
            output_stem: None,
        }];
        job.total = 1;
    }
    upsert_job(&app, job.clone(), true);
    spawn_job_thread(app, job, None);
    Ok(StartDownloadResult { job_id })
}

pub fn resume_download_queue(app: AppHandle) -> Result<(), String> {
    hydrate_jobs(&app);
    let jobs: Vec<queue::DownloadJob> = snapshot_jobs()
        .into_iter()
        .filter(|j| j.is_resumable())
        .collect();
    if jobs.is_empty() {
        return Err("没有可恢复的下载".into());
    }
    if let Ok(settings) = settings::load_settings(&app) {
        SLOT_POOL.set_capacity(settings::clamp_max_concurrent(settings.max_concurrent_downloads) as usize);
    }
    for mut job in jobs {
        if runtime_for(&job.id).is_some() {
            continue;
        }
        if matches!(
            job.status,
            queue::JobStatus::Downloading | queue::JobStatus::Failed
        ) {
            job.status = queue::JobStatus::Pending;
        }
        let resume_items = if job.items.is_empty() {
            None
        } else {
            Some(job.items.clone())
        };
        upsert_job(&app, job.clone(), true);
        spawn_job_thread(app.clone(), job, resume_items);
    }
    Ok(())
}

pub fn discard_download_queue(app: AppHandle) -> Result<(), String> {
    hydrate_jobs(&app);
    let settings = settings::load_settings(&app)?;
    let root = settings::library_root_path(&settings);
    let jobs = snapshot_jobs();
    for job in &jobs {
        if job.is_resumable() {
            let _ = queue::discard_job_temps(&root, job);
        }
    }
    if let Ok(mut map) = JOBS.lock() {
        map.retain(|_, j| !j.is_resumable());
    }
    persist_jobs(&app);
    Ok(())
}

fn run_download(
    app: &AppHandle,
    job_id: &str,
    url: &str,
    category: &str,
    subdir: Option<&str>,
    output_stem: Option<&str>,
    quality: &str,
    audio_only: bool,
    update_last_category: bool,
    allow_size_fallback: bool,
) -> Result<PathBuf, String> {
    if let Ok(settings) = settings::load_settings(app) {
        SLOT_POOL.set_capacity(
            settings::clamp_max_concurrent(settings.max_concurrent_downloads) as usize,
        );
    }
    let rt = runtime_for(job_id).ok_or_else(|| "找不到该下载任务".to_string())?;
    let _slot = SLOT_POOL.acquire(&rt.cancelled)?;
    patch_job(app, job_id, true, |job| {
        if job.status != queue::JobStatus::Cancelled && job.status != queue::JobStatus::Done {
            job.status = queue::JobStatus::Downloading;
        }
        job.detail = "开始下载…".into();
    });

    let mut last_err = None;
    for attempt in 1..=DOWNLOAD_ATTEMPTS {
        if is_job_cancelled(job_id) {
            return Err("已停止下载".into());
        }
        match run_download_once(
            app,
            job_id,
            url,
            category,
            subdir,
            output_stem,
            quality,
            audio_only,
            false,
            allow_size_fallback,
        ) {
            Ok(path) => {
                if update_last_category {
                    let mut settings = settings::load_settings(app)?;
                    settings.last_category = category.to_string();
                    settings::save_settings(app, &settings)?;
                }
                return Ok(path);
            }
            Err(message) if message.contains("已停止") => return Err(message),
            Err(message) => {
                last_err = Some(message);
                if attempt < DOWNLOAD_ATTEMPTS {
                    emit_line(
                        app,
                        job_id,
                        &format!("下载失败，正在重试（{attempt}/{DOWNLOAD_ATTEMPTS}）…"),
                    );
                    std::thread::sleep(Duration::from_secs(2 * u64::from(attempt)));
                }
            }
        }
    }
    Err(last_err.unwrap_or_else(|| "下载失败".into()))
}

fn run_download_once(
    app: &AppHandle,
    job_id: &str,
    url: &str,
    category: &str,
    subdir: Option<&str>,
    output_stem: Option<&str>,
    quality: &str,
    audio_only: bool,
    update_last_category: bool,
    allow_size_fallback: bool,
) -> Result<PathBuf, String> {
    let mut settings = settings::load_settings(app)?;
    let root = settings::library_root_path(&settings);
    library::ensure_library_root(&root)?;
    library::create_category(&root, category).or_else(|e| {
        if e.contains("已存在") {
            Ok(())
        } else {
            Err(e)
        }
    })?;

    let mut out_dir = root.join(category);
    if let Some(sub) = subdir {
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

    let (bin, prefix) = resolve_ytdlp()?;
    let site = site::detect_site(url);
    if site == Site::Missav && !site::is_missav_single_video_url(url) {
        return Err(
            "MissAV 目前仅支持单视频链接，例如 https://missav.ws/cn/番号 或 https://missav.ws/dm127/cn/番号（首页和列表稍后支持）"
                .into(),
        );
    }
    if site == Site::Unknown {
        emit_line(app, job_id, "未识别站点，仍尝试用 yt-dlp 下载…");
    }

    let mut cmd = Command::new(&bin);
    for p in &prefix {
        cmd.arg(p);
    }
    cmd.env("PYTHONUNBUFFERED", "1");
    cmd.env("PYTHONIOENCODING", "utf-8");
    cmd.arg("--newline")
        .arg("--no-playlist")
        .arg("--continue")
        .arg("--progress");
    apply_ytdlp_retry_args(&mut cmd);
    if audio_only {
        cmd.arg("-x")
            .arg("--audio-format")
            .arg("mp3")
            .arg("--audio-quality")
            .arg("0");
    } else {
        cmd.arg("-f")
            .arg(site::format_selector(quality))
            .arg("--merge-output-format")
            .arg("mp4");
    }
    cmd.arg("-o")
        .arg(&template)
        .arg("--print")
        .arg("after_move:filepath")
        .arg("--no-mtime");
    apply_bundled_ffmpeg(&mut cmd);

    if node_available() {
        cmd.arg("--js-runtimes").arg("node");
    }

    let proxy = site::proxy_for(site, &settings);
    if let Some(ref proxy) = proxy {
        cmd.arg("--proxy").arg(proxy);
    }

    if site == Site::Missav {
        let plugin_dirs = ytdlp_plugin_dirs(app);
        if plugin_dirs.is_empty() {
            return Err("未找到 MissAV yt-dlp 插件目录".into());
        }
        for dir in &plugin_dirs {
            cmd.arg("--plugin-dirs").arg(dir);
        }
        // Cloudflare blocks the default client. The bundled yt-dlp can impersonate Safari.
        cmd.arg("--impersonate").arg("Safari-18.0");
    }

    cmd.arg(url);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Own process group so stop can kill yt-dlp + ffmpeg children.
        cmd.process_group(0);
    }

    let site_label = match site {
        Site::Youtube => "YouTube",
        Site::Bilibili => "Bilibili",
        Site::Missav => "MissAV",
        Site::Unknown => "未知站点",
    };
    let proxy_label = proxy
        .as_ref()
        .map(|p| format!("代理 {p}"))
        .unwrap_or_else(|| "直连".into());
    let mode_label = if audio_only {
        "音频模式 · 最好音质 · mp3"
    } else {
        "视频"
    };
    emit_line(
        app,
        job_id,
        &format!("启动 {bin} · {site_label} · {proxy_label} · {mode_label} · 输出 {out_dir:?}"),
    );

    let mut child = cmd.spawn().map_err(|e| format!("启动 yt-dlp 失败: {e}"))?;
    let child_pid = child.id();
    add_child_pid(job_id, child_pid);

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let last_path = Arc::new(std::sync::Mutex::new(None));
    let diag = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let stop_watch = Arc::new(AtomicBool::new(false));

    let app_out = app.clone();
    let job_out = job_id.to_string();
    let path_out = last_path.clone();
    let diag_out = diag.clone();
    let out_handle = std::thread::spawn(move || {
        if let Some(out) = stdout {
            pump_pipe(app_out, job_out, out, path_out, diag_out);
        }
    });

    let app_err = app.clone();
    let job_err = job_id.to_string();
    let path_err = last_path.clone();
    let diag_err = diag.clone();
    let err_handle = std::thread::spawn(move || {
        if let Some(err) = stderr {
            pump_pipe(app_err, job_err, err, path_err, diag_err);
        }
    });

    let app_watch = app.clone();
    let job_watch = job_id.to_string();
    let watch_dir = out_dir.clone();
    let stop_watch2 = stop_watch.clone();
    let watch_handle = std::thread::spawn(move || {
        watch_part_files(app_watch, job_watch, watch_dir, stop_watch2);
    });

    let status = child.wait().map_err(|e| format!("等待进程失败: {e}"))?;
    stop_watch.store(true, Ordering::SeqCst);
    remove_child_pid(job_id, child_pid);
    let _ = out_handle.join();
    let _ = err_handle.join();
    let _ = watch_handle.join();

    if is_job_cancelled(job_id) {
        emit_line(app, job_id, "已停止下载");
        return Err("已停止下载".into());
    }

    if !status.success() {
        let notes = diag.lock().ok().map(|g| g.clone()).unwrap_or_default();
        let detail = format_ytdlp_failure(status.code(), &notes);
        if audio_only {
            return Err(format!("{detail}（音频转码失败时请确认已安装 ffmpeg）"));
        }
        return Err(detail);
    }

    if update_last_category {
        settings.last_category = category.to_string();
        settings::save_settings(app, &settings)?;
    }

    if let Ok(guard) = last_path.lock() {
        if let Some(p) = guard.clone() {
            let path = PathBuf::from(&p);
            if path.exists() {
                if let Err(msg) = validate_playable_output(&path, audio_only) {
                    return Err(msg);
                }
                return Ok(shorten_output_path(&path));
            }
        }
    }

    if !allow_size_fallback {
        return Err("下载完成但未找到输出文件".into());
    }

    let videos = library::list_videos(&root, category)?;
    videos
        .into_iter()
        .max_by_key(|v| v.size)
        .map(|v| shorten_output_path(&PathBuf::from(v.path)))
        .ok_or_else(|| {
            "下载完成但未找到可用成品（可能音视频合并失败；请确认已安装 ffmpeg 后重试）".to_string()
        })
}

fn run_batch_download(
    app: &AppHandle,
    job_id: &str,
    page_url: &str,
    category: &str,
    quality: &str,
    audio_only: bool,
    resume_items: Option<Vec<queue::QueueItem>>,
) -> Result<(), String> {
    let settings = settings::load_settings(app)?;

    let (playlist_items, batch_meta, work_indices) = if let Some(queue_items) = resume_items {
        let mut sorted = queue_items;
        sorted.sort_by_key(|i| i.index);
        let playlist_items: Vec<crate::playlist::PlaylistItem> = sorted
            .iter()
            .map(|i| crate::playlist::PlaylistItem {
                id: i.id.clone(),
                title: i.title.clone(),
                url: Some(i.url.clone()),
                subdir: i.subdir.clone(),
                output_stem: i.output_stem.clone(),
            })
            .collect();
        let batch_meta: Vec<BatchItemMeta> = sorted
            .iter()
            .map(|i| BatchItemMeta {
                id: i.id.clone(),
                title: i.title.clone(),
                status: Some(item_status_label(i.status)),
            })
            .collect();
        let work_indices: Vec<usize> = sorted
            .iter()
            .filter(|i| i.status != queue::ItemStatus::Done)
            .map(|i| i.index)
            .collect();
        (playlist_items, batch_meta, work_indices)
    } else {
        emit_line(app, job_id, "正在解析合集…");
        let prefetched = SEASON_PREFETCH
            .lock()
            .ok()
            .and_then(|mut map| map.remove(job_id));
        let (items, season_title): (Vec<crate::playlist::PlaylistItem>, Option<String>) =
            if let Some(parts) = prefetched {
                let title = parts.first().map(|p| p.season_title.clone());
                let items = parts
                    .into_iter()
                    .map(|p| crate::playlist::PlaylistItem {
                        id: p.id,
                        title: p.title,
                        url: Some(p.url),
                        subdir: Some(p.subdir),
                        output_stem: Some(p.output_stem),
                    })
                    .collect();
                (items, title)
            } else if crate::bilibili::extract_bvid(page_url).is_some() {
                let parts = crate::bilibili::try_expand_ugc_season_from_bv_url(page_url)?
                    .ok_or_else(|| "合集为空或无法解析条目".to_string())?;
                let title = parts.first().map(|p| p.season_title.clone());
                let items = parts
                    .into_iter()
                    .map(|p| crate::playlist::PlaylistItem {
                        id: p.id,
                        title: p.title,
                        url: Some(p.url),
                        subdir: Some(p.subdir),
                        output_stem: Some(p.output_stem),
                    })
                    .collect();
                (items, title)
            } else {
                (
                    crate::playlist::expand_playlist(page_url, &settings, job_id)?,
                    None,
                )
            };
        let batch_meta: Vec<BatchItemMeta> = items
            .iter()
            .map(|i| BatchItemMeta {
                id: i.id.clone(),
                title: i.title.clone(),
                status: Some("pending".into()),
            })
            .collect();
        let queue_items: Vec<queue::QueueItem> = items
            .iter()
            .enumerate()
            .map(|(index, i)| queue::QueueItem {
                index,
                id: i.id.clone(),
                title: i.title.clone(),
                url: crate::playlist::resolve_item_url(page_url, i),
                status: queue::ItemStatus::Pending,
                subdir: i.subdir.clone(),
                output_stem: i.output_stem.clone(),
            })
            .collect();
        let title = if let Some(season) = season_title {
            format!("{} · {} 集", season, items.len())
        } else {
            items
                .first()
                .map(|i| {
                    if items.len() > 1 {
                        format!("{} 等 {} 个", i.title, items.len())
                    } else {
                        i.title.clone()
                    }
                })
                .unwrap_or_else(|| page_url.to_string())
        };
        patch_job(app, job_id, true, |job| {
            job.items = queue_items;
            job.total = items.len();
            job.completed = 0;
            job.title = title;
            job.kind = queue::QueueKind::Batch;
        });
        let work_indices: Vec<usize> = (0..items.len()).collect();
        (items, batch_meta, work_indices)
    };

    let total = playlist_items.len();
    let _ = app.emit(
        "download-batch-started",
        DownloadBatchStarted {
            job_id: job_id.to_string(),
            total,
            items: batch_meta,
        },
    );
    patch_job(app, job_id, true, |job| {
        job.total = total;
        job.status = queue::JobStatus::Downloading;
        job.percent = Some(batch_overall_percent(job.completed, total.max(1), Some(0.0)));
    });

    let configured = settings::clamp_max_concurrent(settings.max_concurrent_downloads) as usize;
    let concurrency = if site::detect_site(page_url) == Site::Youtube {
        let n = youtube_batch_concurrency(settings.max_concurrent_downloads);
        if n < configured {
            emit_line(
                app,
                job_id,
                &format!("YouTube 合集并发限制为 {n}（避免代理/限流导致失败），失败项会自动重试"),
            );
        }
        n
    } else {
        configured
    };
    let next = Arc::new(AtomicUsize::new(0));
    let succeeded = Arc::new(AtomicUsize::new(0));
    let failed = Arc::new(AtomicUsize::new(0));
    let items = Arc::new(playlist_items);
    let todo = Arc::new(work_indices);

    let worker_count = concurrency.max(1).min(todo.len().max(1));
    let mut handles = Vec::with_capacity(worker_count);

    for _ in 0..worker_count {
        let app = app.clone();
        let job_id = job_id.to_string();
        let next = next.clone();
        let items = items.clone();
        let todo = todo.clone();
        let succeeded = succeeded.clone();
        let failed = failed.clone();
        let category = category.to_string();
        let quality = quality.to_string();
        let page_url = page_url.to_string();
        handles.push(std::thread::spawn(move || loop {
            if is_job_cancelled(&job_id) {
                break;
            }
            let pos = next.fetch_add(1, Ordering::SeqCst);
            if pos >= todo.len() {
                break;
            }
            let index = todo[pos];
            if index >= items.len() {
                continue;
            }
            let item = &items[index];
            persist_item_status(&app, &job_id, index, queue::ItemStatus::Downloading);
            let _ = app.emit(
                "download-item-started",
                DownloadItemStarted {
                    job_id: job_id.clone(),
                    index,
                    id: item.id.clone(),
                },
            );
            let url = crate::playlist::resolve_item_url(&page_url, item);
            match run_download(
                &app,
                &job_id,
                &url,
                &category,
                item.subdir.as_deref(),
                item.output_stem.as_deref(),
                &quality,
                audio_only,
                false,
                false,
            ) {
                Ok(path) => {
                    succeeded.fetch_add(1, Ordering::SeqCst);
                    persist_item_status(&app, &job_id, index, queue::ItemStatus::Done);
                    patch_job(&app, &job_id, true, |job| {
                        job.percent = Some(batch_overall_percent(job.completed, job.total, Some(0.0)));
                    });
                    let _ = app.emit(
                        "download-item-finished",
                        DownloadItemFinished {
                            job_id: job_id.clone(),
                            index,
                            id: item.id.clone(),
                            path: path.to_string_lossy().into_owned(),
                        },
                    );
                }
                Err(message) => {
                    if message.contains("已停止") {
                        continue;
                    }
                    failed.fetch_add(1, Ordering::SeqCst);
                    persist_item_status(&app, &job_id, index, queue::ItemStatus::Failed);
                    let _ = app.emit(
                        "download-item-error",
                        DownloadItemError {
                            job_id: job_id.clone(),
                            index,
                            id: item.id.clone(),
                            message,
                        },
                    );
                }
            }
        }));
    }

    for h in handles {
        let _ = h.join();
    }

    let succeeded_n = succeeded.load(Ordering::SeqCst);
    let failed_n = failed.load(Ordering::SeqCst);
    let cancelled = if is_job_cancelled(job_id) {
        total.saturating_sub(succeeded_n + failed_n)
    } else {
        0
    };
    let _ = app.emit(
        "download-batch-finished",
        DownloadBatchFinished {
            job_id: job_id.to_string(),
            succeeded: succeeded_n,
            failed: failed_n,
            cancelled,
        },
    );

    let mut settings = settings::load_settings(app)?;
    settings.last_category = category.to_string();
    settings::save_settings(app, &settings)?;

    Ok(())
}

fn shorten_output_path(path: &Path) -> PathBuf {
    match library::shorten_downloaded_filename(path) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("缩短文件名失败，保留原名: {e}");
            path.to_path_buf()
        }
    }
}

fn validate_playable_output(path: &Path, audio_only: bool) -> Result<(), String> {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    if library::looks_like_ytdlp_fragment(name) {
        return Err(format!(
            "下载未合并完成（残留分片 {name}）。请确认已安装 ffmpeg，并重新下载。"
        ));
    }
    if audio_only {
        return Ok(());
    }
    // Soft check via ffprobe when available: require a video stream.
    let Ok(out) = Command::new(ffprobe_program())
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=codec_name",
            "-of",
            "csv=p=0",
            path.to_str().unwrap_or(""),
        ])
        .output()
    else {
        return Ok(());
    };
    if !out.status.success() || String::from_utf8_lossy(&out.stdout).trim().is_empty() {
        return Err("成品缺少视频轨，下载可能不完整，请重试。".into());
    }
    let audio = Command::new(ffprobe_program())
        .args([
            "-v",
            "error",
            "-select_streams",
            "a:0",
            "-show_entries",
            "stream=codec_name",
            "-of",
            "csv=p=0",
            path.to_str().unwrap_or(""),
        ])
        .output();
    if let Ok(a) = audio {
        if a.status.success() && String::from_utf8_lossy(&a.stdout).trim().is_empty() {
            return Err("成品没有音轨（常见于合并失败）。请确认 ffmpeg 可用后重新下载。".into());
        }
    }
    Ok(())
}

pub fn is_download_running() -> bool {
    RUNTIMES
        .lock()
        .ok()
        .map(|g| !g.is_empty())
        .unwrap_or(false)
        || snapshot_jobs().iter().any(|j| j.status.is_active())
}

fn queue_item_id_from_url(url: &str) -> String {
    let base = url
        .split('?')
        .next()
        .unwrap_or(url)
        .split('#')
        .next()
        .unwrap_or(url);
    base.rsplit('/')
        .find(|s| !s.is_empty())
        .unwrap_or(url)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_download_percent() {
        let line = "[download]  45.2% of  237.23MiB at    7.54MiB/s ETA 00:00";
        assert_eq!(parse_percent(line), Some(45.2));
        assert_eq!(parse_percent("hello"), None);
    }

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

    #[test]
    fn queue_item_id_from_url_strips_query_and_uses_last_segment() {
        assert_eq!(
            queue_item_id_from_url("https://www.bilibili.com/video/BV1xx411c7mD?p=1"),
            "BV1xx411c7mD"
        );
        assert_eq!(
            queue_item_id_from_url("https://example.com/watch/abc123#t=10"),
            "abc123"
        );
    }

    #[test]
    fn missav_plugin_file_exists() {
        assert!(
            missav_plugin_file().is_file(),
            "expected MissAV yt-dlp plugin at {:?}",
            missav_plugin_file()
        );
    }

    #[test]
    fn youtube_batch_concurrency_caps_at_two() {
        assert_eq!(youtube_batch_concurrency(1), 1);
        assert_eq!(youtube_batch_concurrency(2), 2);
        assert_eq!(youtube_batch_concurrency(5), 2);
        assert_eq!(youtube_batch_concurrency(10), 2);
    }

    #[test]
    fn ytdlp_failure_prefers_error_line() {
        assert!(is_ytdlp_diag_line(
            "ERROR: unable to download video data: HTTP Error 403: Forbidden"
        ));
        assert!(is_ytdlp_diag_line(
            "WARNING: [youtube:tab] [Errno 111] Connection refused"
        ));
        assert!(!is_ytdlp_diag_line("[download]  45.2% of 10.00MiB"));
        let notes = vec![
            "WARNING: something".into(),
            "ERROR: unable to download video data: HTTP Error 403: Forbidden".into(),
        ];
        let msg = format_ytdlp_failure(Some(1), &notes);
        assert!(msg.contains("403"));
        assert!(msg.contains("退出码"));
    }

    #[test]
    fn batch_overall_percent_folds_current_item() {
        assert_eq!(batch_overall_percent(3, 10, Some(50.0)), 35.0);
        assert_eq!(batch_overall_percent(0, 1, Some(40.0)), 40.0);
        assert_eq!(batch_overall_percent(0, 0, Some(10.0)), 0.0);
        assert_eq!(batch_overall_percent(2, 4, None), 50.0);
    }

    #[test]
    fn slot_pool_second_acquire_waits_until_release() {
        let pool = Arc::new(SlotPool::new());
        pool.set_capacity(1);
        let flag = AtomicBool::new(false);
        let first = pool.acquire(&flag).unwrap();
        let started = Arc::new(AtomicBool::new(false));
        let finished = Arc::new(AtomicBool::new(false));
        let started2 = started.clone();
        let finished2 = finished.clone();
        let pool2 = pool.clone();
        let handle = std::thread::spawn(move || {
            let flag = AtomicBool::new(false);
            started2.store(true, Ordering::SeqCst);
            let _g = pool2.acquire(&flag).unwrap();
            finished2.store(true, Ordering::SeqCst);
        });
        std::thread::sleep(Duration::from_millis(80));
        assert!(started.load(Ordering::SeqCst));
        assert!(!finished.load(Ordering::SeqCst));
        drop(first);
        handle.join().unwrap();
        assert!(finished.load(Ordering::SeqCst));
    }

    #[test]
    fn cancel_job_a_does_not_cancel_job_b() {
        let a = format!("job-test-a-{}", std::process::id());
        let b = format!("job-test-b-{}", std::process::id());
        install_runtime(&a);
        install_runtime(&b);
        let _ = cancel_job_runtime(&a);
        assert!(is_job_cancelled(&a));
        assert!(!is_job_cancelled(&b));
        remove_runtime(&a);
        remove_runtime(&b);
    }
}
