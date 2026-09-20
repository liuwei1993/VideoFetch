use crate::library::{self, UNCATEGORIZED};
use crate::settings::{self};
use crate::site::{self, Site};
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashSet;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

#[derive(Clone, Serialize)]
pub struct DownloadProgress {
    pub percent: Option<f64>,
    pub line: String,
    pub speed: Option<String>,
    pub eta: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct DownloadFinished {
    pub path: String,
}

#[derive(Clone, Serialize)]
pub struct DownloadError {
    pub message: String,
}

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

static DOWNLOAD_RUNNING: AtomicBool = AtomicBool::new(false);
static DOWNLOAD_CANCELLED: AtomicBool = AtomicBool::new(false);
static CHILD_PIDS: LazyLock<Mutex<HashSet<u32>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

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

pub fn resolve_ytdlp() -> Result<(String, Vec<String>), String> {
    if let Ok(custom) = std::env::var("VIDEOFETCH_YTDLP") {
        if !custom.trim().is_empty() {
            return Ok((custom, vec![]));
        }
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
    Err("未找到 yt-dlp。请安装 uv（推荐）或把 yt-dlp 加入 PATH，也可设置 VIDEOFETCH_YTDLP。".into())
}

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

fn emit_line(app: &AppHandle, line: &str) {
    let line = line.trim();
    if line.is_empty() {
        return;
    }
    let percent = parse_percent(line);
    let speed = parse_speed(line);
    let eta = parse_eta(line);
    let path_hint = !line.starts_with('[') && Path::new(line).is_absolute();
    let _ = app.emit(
        "download-progress",
        DownloadProgress {
            percent,
            line: line.to_string(),
            speed,
            eta,
        },
    );
    let _ = path_hint; // handled by caller collecting last_path
}

/// Read pipe bytes and split on both `\n` and `\r` (yt-dlp progress often uses `\r`).
fn pump_pipe(app: AppHandle, mut pipe: impl Read, last_path: Arc<std::sync::Mutex<Option<String>>>) {
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
                            emit_line(&app, &line);
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
        emit_line(&app, &line);
    }
}

fn watch_part_files(app: AppHandle, out_dir: PathBuf, stop: Arc<AtomicBool>) {
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
            let _ = app.emit(
                "download-progress",
                DownloadProgress {
                    percent: None,
                    line: format!("写入中 {mb:.1} MB · {name}"),
                    speed: None,
                    eta: None,
                },
            );
        }
    }
}

pub fn start_download(app: AppHandle, args: StartDownloadArgs) -> Result<(), String> {
    if DOWNLOAD_RUNNING.swap(true, Ordering::SeqCst) {
        return Err("已有下载任务在进行".into());
    }
    DOWNLOAD_CANCELLED.store(false, Ordering::SeqCst);

    let url = args.url.trim().to_string();
    if url.is_empty() {
        DOWNLOAD_RUNNING.store(false, Ordering::SeqCst);
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

    let app_for_job = app.clone();
    std::thread::spawn(move || {
        let is_batch = site::is_bilibili_collection_url(&url);
        let session = (|| -> Result<SessionOutcome, String> {
            if is_batch {
                run_batch_download(
                    &app_for_job,
                    &url,
                    &category,
                    &quality,
                    audio_only,
                )?;
                Ok(SessionOutcome::Batch)
            } else {
                Ok(SessionOutcome::Single(run_download(
                    &app_for_job,
                    &url,
                    &category,
                    &quality,
                    audio_only,
                )?))
            }
        })();
        clear_child_pids();
        DOWNLOAD_RUNNING.store(false, Ordering::SeqCst);
        match session {
            Ok(SessionOutcome::Batch) => {}
            Ok(SessionOutcome::Single(path)) => {
                let _ = app_for_job.emit(
                    "download-finished",
                    DownloadFinished {
                        path: path.to_string_lossy().into_owned(),
                    },
                );
            }
            Err(message) => {
                let _ = app_for_job.emit("download-error", DownloadError { message });
            }
        }
    });

    Ok(())
}

fn run_download(
    app: &AppHandle,
    url: &str,
    category: &str,
    quality: &str,
    audio_only: bool,
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

    let out_dir = root.join(category);
    let template = out_dir
        .join("%(title)s [%(id)s].%(ext)s")
        .to_string_lossy()
        .into_owned();

    let (bin, prefix) = resolve_ytdlp()?;
    let site = site::detect_site(url);
    if site == Site::Unknown {
        emit_line(app, "未识别站点，仍尝试用 yt-dlp 下载…");
    }

    let mut cmd = Command::new(&bin);
    for p in &prefix {
        cmd.arg(p);
    }
    cmd.env("PYTHONUNBUFFERED", "1");
    cmd.env("PYTHONIOENCODING", "utf-8");
    cmd.arg("--newline")
        .arg("--no-playlist")
        .arg("--progress");
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

    if node_available() {
        cmd.arg("--js-runtimes").arg("node");
    }

    let proxy = site::proxy_for(site, &settings);
    if let Some(ref proxy) = proxy {
        cmd.arg("--proxy").arg(proxy);
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
    let _ = app.emit(
        "download-progress",
        DownloadProgress {
            percent: Some(0.0),
            line: format!(
                "启动 {bin} · {site_label} · {proxy_label} · {mode_label} · 输出 {out_dir:?}"
            ),
            speed: None,
            eta: None,
        },
    );

    let mut child = cmd.spawn().map_err(|e| format!("启动 yt-dlp 失败: {e}"))?;
    let child_pid = child.id();
    add_child_pid(child_pid);

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let last_path = Arc::new(std::sync::Mutex::new(None));
    let stop_watch = Arc::new(AtomicBool::new(false));

    let app_out = app.clone();
    let path_out = last_path.clone();
    let out_handle = std::thread::spawn(move || {
        if let Some(out) = stdout {
            pump_pipe(app_out, out, path_out);
        }
    });

    let app_err = app.clone();
    let path_err = last_path.clone();
    let err_handle = std::thread::spawn(move || {
        if let Some(err) = stderr {
            pump_pipe(app_err, err, path_err);
        }
    });

    let app_watch = app.clone();
    let watch_dir = out_dir.clone();
    let stop_watch2 = stop_watch.clone();
    let watch_handle = std::thread::spawn(move || {
        watch_part_files(app_watch, watch_dir, stop_watch2);
    });

    let status = child.wait().map_err(|e| format!("等待进程失败: {e}"))?;
    stop_watch.store(true, Ordering::SeqCst);
    remove_child_pid(child_pid);
    let _ = out_handle.join();
    let _ = err_handle.join();
    let _ = watch_handle.join();

    if DOWNLOAD_CANCELLED.load(Ordering::SeqCst) {
        let _ = app.emit(
            "download-progress",
            DownloadProgress {
                percent: None,
                line: "已停止下载".into(),
                speed: None,
                eta: None,
            },
        );
        return Err("已停止下载".into());
    }

    if !status.success() {
        if audio_only {
            return Err(format!(
                "yt-dlp 退出码: {:?}（音频转码失败时请确认已安装 ffmpeg）",
                status.code()
            ));
        }
        return Err(format!("yt-dlp 退出码: {:?}", status.code()));
    }

    settings.last_category = category.to_string();
    settings::save_settings(app, &settings)?;

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
    let next = Arc::new(AtomicUsize::new(0));
    let succeeded = Arc::new(AtomicUsize::new(0));
    let failed = Arc::new(AtomicUsize::new(0));
    let items = Arc::new(items);

    let worker_count = concurrency.max(1).min(total);
    let mut handles = Vec::with_capacity(worker_count);

    for _ in 0..worker_count {
        let app = app.clone();
        let next = next.clone();
        let items = items.clone();
        let succeeded = succeeded.clone();
        let failed = failed.clone();
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
                        if message.contains("已停止") {
                            continue;
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
    let cancelled = if DOWNLOAD_CANCELLED.load(Ordering::SeqCst) {
        total.saturating_sub(succeeded_n + failed_n)
    } else {
        0
    };
    let _ = app.emit(
        "download-batch-finished",
        DownloadBatchFinished {
            succeeded: succeeded_n,
            failed: failed_n,
            cancelled,
        },
    );
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
    let Ok(out) = Command::new("ffprobe")
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
    let audio = Command::new("ffprobe")
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
            return Err(
                "成品没有音轨（常见于合并失败）。请确认 ffmpeg 可用后重新下载。".into(),
            );
        }
    }
    Ok(())
}

pub fn is_download_running() -> bool {
    DOWNLOAD_RUNNING.load(Ordering::SeqCst)
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
}
