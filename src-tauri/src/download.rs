use crate::library::{self, UNCATEGORIZED};
use crate::settings::{self};
use crate::site::{self, Site};
use serde::Deserialize;
use serde::Serialize;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

#[derive(Clone, Serialize)]
pub struct DownloadProgress {
    pub percent: Option<f64>,
    pub line: String,
}

#[derive(Clone, Serialize)]
pub struct DownloadFinished {
    pub path: String,
}

#[derive(Clone, Serialize)]
pub struct DownloadError {
    pub message: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartDownloadArgs {
    pub url: String,
    pub category: String,
    pub quality: String,
}

static DOWNLOAD_RUNNING: AtomicBool = AtomicBool::new(false);

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

fn emit_line(app: &AppHandle, line: &str) {
    let line = line.trim();
    if line.is_empty() {
        return;
    }
    let percent = parse_percent(line);
    let path_hint = !line.starts_with('[') && Path::new(line).is_absolute();
    let _ = app.emit(
        "download-progress",
        DownloadProgress {
            percent,
            line: line.to_string(),
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
                },
            );
        }
    }
}

pub fn start_download(app: AppHandle, args: StartDownloadArgs) -> Result<(), String> {
    if DOWNLOAD_RUNNING.swap(true, Ordering::SeqCst) {
        return Err("已有下载任务在进行".into());
    }

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

    let app_for_job = app.clone();
    std::thread::spawn(move || {
        let result = run_download(&app_for_job, &url, &category, &quality);
        DOWNLOAD_RUNNING.store(false, Ordering::SeqCst);
        match result {
            Ok(path) => {
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
        .arg("--progress")
        .arg("-f")
        .arg(site::format_selector(quality))
        .arg("--merge-output-format")
        .arg("mp4")
        .arg("-o")
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

    let site_label = match site {
        Site::Youtube => "YouTube",
        Site::Bilibili => "Bilibili",
        Site::Unknown => "未知站点",
    };
    let proxy_label = proxy
        .as_ref()
        .map(|p| format!("代理 {p}"))
        .unwrap_or_else(|| "直连".into());
    let _ = app.emit(
        "download-progress",
        DownloadProgress {
            percent: Some(0.0),
            line: format!("启动 {bin} · {site_label} · {proxy_label} · 输出 {out_dir:?}"),
        },
    );

    let mut child = cmd.spawn().map_err(|e| format!("启动 yt-dlp 失败: {e}"))?;

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
    let _ = out_handle.join();
    let _ = err_handle.join();
    let _ = watch_handle.join();

    if !status.success() {
        return Err(format!("yt-dlp 退出码: {:?}", status.code()));
    }

    settings.last_category = category.to_string();
    settings::save_settings(app, &settings)?;

    if let Ok(guard) = last_path.lock() {
        if let Some(p) = guard.clone() {
            let path = PathBuf::from(&p);
            if path.exists() {
                return Ok(path);
            }
        }
    }

    let videos = library::list_videos(&root, category)?;
    videos
        .into_iter()
        .max_by_key(|v| v.size)
        .map(|v| PathBuf::from(v.path))
        .ok_or_else(|| "下载完成但未找到输出文件".to_string())
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
}
