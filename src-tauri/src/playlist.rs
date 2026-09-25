use crate::download;
use crate::settings::Settings;
use crate::site;
use serde::Serialize;
use std::io::Read;
use std::process::{Command, Stdio};

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

pub fn bilibili_video_url(id: &str) -> String {
    format!("https://www.bilibili.com/video/{id}")
}

pub fn item_video_url(page_url: &str, id: &str) -> String {
    match site::detect_site(page_url) {
        site::Site::Youtube => format!("https://www.youtube.com/watch?v={id}"),
        _ => bilibili_video_url(id),
    }
}

pub fn resolve_item_url(page_url: &str, item: &PlaylistItem) -> String {
    if let Some(ref u) = item.url {
        if !u.is_empty() {
            return u.clone();
        }
    }
    item_video_url(page_url, &item.id)
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
            url: None,
            subdir: None,
            output_stem: None,
        });
    }
    items
}

/// Run yt-dlp --flat-playlist and return items. Applies Bilibili proxy from settings.
pub fn expand_playlist(url: &str, settings: &Settings, job_id: &str) -> Result<Vec<PlaylistItem>, String> {
    let (bin, prefix) = download::resolve_ytdlp()?;
    let mut cmd = Command::new(&bin);
    for p in &prefix {
        cmd.arg(p);
    }
    cmd.env("PYTHONUNBUFFERED", "1");
    cmd.env("PYTHONIOENCODING", "utf-8");
    cmd.arg("--flat-playlist")
        .arg("--print")
        .arg("%(id)s\t%(title)s");
    download::apply_ytdlp_retry_args(&mut cmd);
    download::apply_bundled_ffmpeg(&mut cmd);

    let site = site::detect_site(url);
    if let Some(proxy) = site::proxy_for(site, settings) {
        cmd.arg("--proxy").arg(proxy);
    }

    cmd.arg(url);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("展开合集失败（启动 yt-dlp）: {e}"))?;
    let child_pid = child.id();
    download::add_child_pid(job_id, child_pid);

    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();
    let stdout_handle = std::thread::spawn(move || {
        let mut buf = String::new();
        if let Some(mut out) = stdout_pipe {
            let _ = out.read_to_string(&mut buf);
        }
        buf
    });
    let stderr_handle = std::thread::spawn(move || {
        let mut buf = String::new();
        if let Some(mut err) = stderr_pipe {
            let _ = err.read_to_string(&mut buf);
        }
        buf
    });

    let status = child
        .wait()
        .map_err(|e| format!("展开合集失败（等待 yt-dlp）: {e}"))?;
    download::remove_child_pid(job_id, child_pid);

    let stdout = stdout_handle.join().unwrap_or_default();
    let stderr = stderr_handle.join().unwrap_or_default();

    if download::is_job_cancelled(job_id) {
        return Err("已停止下载".into());
    }

    if !status.success() {
        return Err(format!(
            "展开合集失败: {} {}",
            status,
            stderr.chars().take(400).collect::<String>()
        ));
    }
    let items = parse_flat_playlist_output(&stdout);
    if items.is_empty() {
        return Err("合集为空或无法解析条目".into());
    }
    Ok(items)
}

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
        assert_eq!(
            item_video_url("https://space.bilibili.com/1/lists/2", "BV1xx"),
            "https://www.bilibili.com/video/BV1xx"
        );
        assert_eq!(
            item_video_url(
                "https://www.youtube.com/@bruce_lu_1993/videos",
                "s_1DlVFOPIA"
            ),
            "https://www.youtube.com/watch?v=s_1DlVFOPIA"
        );
    }

    #[test]
    fn resolve_item_url_prefers_explicit() {
        let item = PlaylistItem {
            id: "BV1xx_p2".into(),
            title: "p2".into(),
            url: Some("https://www.bilibili.com/video/BV1xx?p=2".into()),
            subdir: None,
            output_stem: None,
        };
        assert_eq!(
            resolve_item_url("https://www.bilibili.com/video/BV1xx", &item),
            "https://www.bilibili.com/video/BV1xx?p=2"
        );
    }
}
