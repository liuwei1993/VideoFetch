use crate::download;
use crate::settings::Settings;
use crate::site;
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
    cmd.env("PYTHONUNBUFFERED", "1");
    cmd.env("PYTHONIOENCODING", "utf-8");
    cmd.arg("--flat-playlist")
        .arg("--print")
        .arg("%(id)s\t%(title)s");

    let site = site::detect_site(url);
    if let Some(proxy) = site::proxy_for(site, settings) {
        cmd.arg("--proxy").arg(proxy);
    }

    cmd.arg(url);
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
    }
}
