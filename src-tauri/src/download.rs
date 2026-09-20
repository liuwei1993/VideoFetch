use crate::library::{self, UNCATEGORIZED};
use crate::settings::{self};
use crate::site::{self, Site};
use serde::Serialize;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
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

use serde::Deserialize;

static DOWNLOAD_RUNNING: AtomicBool = AtomicBool::new(false);

pub fn resolve_ytdlp() -> Result<(String, Vec<String>), String> {
    if let Ok(custom) = std::env::var("VIDEOFETCH_YTDLP") {
        if !custom.trim().is_empty() {
            return Ok((custom, vec![]));
        }
    }
    if command_exists("uvx") {
        return Ok((
            "uvx".into(),
            vec![
                "--from".into(),
                "yt-dlp".into(),
                "yt-dlp".into(),
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

    tauri::async_runtime::spawn(async move {
        let result = run_download(&app, &url, &category, &quality).await;
        DOWNLOAD_RUNNING.store(false, Ordering::SeqCst);
        match result {
            Ok(path) => {
                let _ = app.emit(
                    "download-finished",
                    DownloadFinished {
                        path: path.to_string_lossy().into_owned(),
                    },
                );
            }
            Err(message) => {
                let _ = app.emit("download-error", DownloadError { message });
            }
        }
    });

    Ok(())
}

async fn run_download(
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
        let _ = app.emit(
            "download-progress",
            DownloadProgress {
                percent: None,
                line: "未识别站点，仍尝试用 yt-dlp 下载…".into(),
            },
        );
    }

    let mut cmd = Command::new(&bin);
    for p in &prefix {
        cmd.arg(p);
    }
    cmd.arg("--newline")
        .arg("--no-playlist")
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

    if let Some(proxy) = site::proxy_for(site, &settings) {
        cmd.arg("--proxy").arg(proxy);
    }

    cmd.arg(url);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let _ = app.emit(
        "download-progress",
        DownloadProgress {
            percent: Some(0.0),
            line: format!("启动: {bin} …"),
        },
    );

    let mut child = cmd.spawn().map_err(|e| format!("启动 yt-dlp 失败: {e}"))?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let app2 = app.clone();
    let app3 = app.clone();

    let out_handle = std::thread::spawn(move || {
        let mut last_path: Option<String> = None;
        if let Some(out) = stdout {
            let reader = BufReader::new(out);
            for line in reader.lines().flatten() {
                let percent = parse_percent(&line);
                // after_move:filepath prints a bare path line sometimes
                if !line.starts_with('[') && Path::new(&line).is_absolute() {
                    last_path = Some(line.clone());
                }
                let _ = app2.emit(
                    "download-progress",
                    DownloadProgress {
                        percent,
                        line: line.clone(),
                    },
                );
            }
        }
        last_path
    });

    let err_handle = std::thread::spawn(move || {
        if let Some(err) = stderr {
            let reader = BufReader::new(err);
            for line in reader.lines().flatten() {
                let percent = parse_percent(&line);
                let _ = app3.emit(
                    "download-progress",
                    DownloadProgress {
                        percent,
                        line: line.clone(),
                    },
                );
            }
        }
    });

    let status = child.wait().map_err(|e| format!("等待进程失败: {e}"))?;
    let printed_path = out_handle.join().ok().flatten();
    let _ = err_handle.join();

    if !status.success() {
        return Err(format!("yt-dlp 退出码: {:?}", status.code()));
    }

    settings.last_category = category.to_string();
    settings::save_settings(app, &settings)?;

    if let Some(p) = printed_path {
        let path = PathBuf::from(&p);
        if path.exists() {
            return Ok(path);
        }
    }

    // Fallback: newest video file in category dir
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
