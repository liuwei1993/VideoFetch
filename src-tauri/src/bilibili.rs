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

fn ugc_season_present(json: &str) -> Result<bool, String> {
    let root: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("invalid JSON: {e}"))?;

    if let Some(code) = root.get("code").and_then(|c| c.as_i64()) {
        if code != 0 {
            return Err(format!("bilibili API code {code}"));
        }
    }

    Ok(root
        .get("data")
        .and_then(|d| d.get("ugc_season"))
        .map(|s| !s.is_null())
        .unwrap_or(false))
}

/// Returns Some(parts) if ugc_season present and non-empty; None if no season key.
/// If ugc_season key is present but expands to 0 parts → Err (not Ok(None)).
pub fn try_expand_ugc_season_from_bv_url(page_url: &str) -> Result<Option<Vec<SeasonPart>>, String> {
    let Some(bvid) = extract_bvid(page_url) else {
        return Ok(None);
    };
    let json = fetch_view_json(&bvid)?;
    if !ugc_season_present(&json)? {
        return Ok(None);
    }
    let items = parse_ugc_season_items(&json)?;
    if items.is_empty() {
        return Err("合集为空或无法解析条目".to_string());
    }
    Ok(Some(items))
}

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeasonPart {
    pub id: String,
    pub title: String,
    pub url: String,
    pub subdir: String,
    pub output_stem: String,
    pub season_title: String,
}

pub fn parse_ugc_season_items(json: &str) -> Result<Vec<SeasonPart>, String> {
    let root: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("invalid JSON: {e}"))?;

    if let Some(code) = root.get("code").and_then(|c| c.as_i64()) {
        if code != 0 {
            return Err(format!("bilibili API code {code}"));
        }
    }

    let ugc_season = match root.get("data").and_then(|d| d.get("ugc_season")) {
        Some(s) if !s.is_null() => s,
        _ => return Ok(vec![]),
    };

    let season_title = ugc_season
        .get("title")
        .and_then(|t| t.as_str())
        .unwrap_or("untitled")
        .to_string();
    let season_dir = sanitize_path_component(&season_title);

    let sections = ugc_season
        .get("sections")
        .and_then(|s| s.as_array())
        .ok_or("ugc_season.sections missing or not an array")?;

    let mut items = Vec::new();

    for section in sections {
        let episodes = section.get("episodes").and_then(|e| e.as_array());
        let episodes = episodes.map(|s| s.as_slice()).unwrap_or(&[]);

        for episode in episodes {
            let bvid = episode
                .get("bvid")
                .and_then(|b| b.as_str())
                .ok_or("episode missing bvid")?;
            let episode_title = episode
                .get("title")
                .and_then(|t| t.as_str())
                .unwrap_or("untitled");
            let episode_dir = sanitize_path_component(episode_title);
            let subdir = format!("{}/{}", season_dir, episode_dir);

            let pages = episode.get("pages").and_then(|p| p.as_array());
            if let Some(pages) = pages {
                if pages.is_empty() {
                    push_season_part(
                        &mut items,
                        bvid,
                        1,
                        episode_title,
                        &subdir,
                        &season_title,
                    );
                } else {
                    for page in pages {
                        let page_num = page
                            .get("page")
                            .and_then(|p| p.as_u64())
                            .ok_or("page missing page number")? as u32;
                        let part_title = page
                            .get("part")
                            .and_then(|p| p.as_str())
                            .unwrap_or(episode_title);
                        push_season_part(
                            &mut items,
                            bvid,
                            page_num,
                            part_title,
                            &subdir,
                            &season_title,
                        );
                    }
                }
            } else {
                push_season_part(
                    &mut items,
                    bvid,
                    1,
                    episode_title,
                    &subdir,
                    &season_title,
                );
            }
        }
    }

    Ok(items)
}

fn push_season_part(
    items: &mut Vec<SeasonPart>,
    bvid: &str,
    page: u32,
    part_title: &str,
    subdir: &str,
    season_title: &str,
) {
    let id = format!("{bvid}_p{page}");
    let title = part_title.to_string();
    let url = format!("https://www.bilibili.com/video/{bvid}?p={page}");
    let sanitized_part = sanitize_path_component(part_title);
    let output_stem = format!("{sanitized_part} [{id}]");
    items.push(SeasonPart {
        id,
        title,
        url,
        subdir: subdir.to_string(),
        output_stem,
        season_title: season_title.to_string(),
    });
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn extract_bvid_from_video_url() {
        assert_eq!(
            extract_bvid("https://www.bilibili.com/video/BV1NCgVzoEG9/?spm_id_from=333.788"),
            Some("BV1NCgVzoEG9".into())
        );
        assert_eq!(
            extract_bvid("https://www.bilibili.com/video/bv1ncgvzoeg9"),
            Some("BV1ncgvzoeg9".into())
        );
        assert_eq!(extract_bvid("https://space.bilibili.com/1/lists/2"), None);
    }

    #[test]
    fn sanitize_path_component_strips_illegal() {
        assert_eq!(sanitize_path_component("A/B:C*"), "A_B_C_");
        assert_eq!(sanitize_path_component("  hi  "), "hi");
        assert!(sanitize_path_component(&"x".repeat(200)).chars().count() <= 80);
    }

    #[test]
    fn parse_ugc_season_expands_all_pages() {
        let raw = include_str!("../tests/fixtures/bilibili_view_ugc_season.json");
        let items = parse_ugc_season_items(raw).unwrap();
        assert_eq!(items.len(), 4);
        assert_eq!(items[0].id, "BV1NCgVzoEG9_p1");
        assert_eq!(items[0].title, "01 从函数到神经网络");
        assert_eq!(items[0].url, "https://www.bilibili.com/video/BV1NCgVzoEG9?p=1");
        assert_eq!(items[0].subdir, "AI入门/【完整合集】一小时从函数到Transformer！");
        assert_eq!(items[0].output_stem, "01 从函数到神经网络 [BV1NCgVzoEG9_p1]");
        assert_eq!(items[2].id, "BV15z4C6SEHT_p1");
    }

    #[test]
    fn parse_view_without_season_returns_empty() {
        let raw = r#"{"code":0,"data":{"bvid":"BV1xx","title":"solo","pages":[{"page":1,"part":"p1"}]}}"#;
        assert!(parse_ugc_season_items(raw).unwrap().is_empty());
    }

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

    /// End-to-end smoke: expand season and download first part of each episode.
    #[test]
    #[ignore]
    fn live_download_two_season_parts() {
        use std::process::Command;

        let parts = try_expand_ugc_season_from_bv_url(
            "https://www.bilibili.com/video/BV1NCgVzoEG9/",
        )
        .unwrap()
        .expect("season");
        assert!(parts.len() >= 17);

        let first_a = parts
            .iter()
            .find(|p| p.id.starts_with("BV1NCgVzoEG9"))
            .expect("ep1");
        let first_b = parts
            .iter()
            .find(|p| p.id.starts_with("BV15z4C6SEHT"))
            .expect("ep2");

        let root = std::env::temp_dir().join(format!(
            "videofetch_ugc_dl_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        let ytdlp = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("binaries")
            .join(format!("yt-dlp-{}", env!("VIDEOFETCH_HOST_TRIPLE")));
        assert!(
            ytdlp.exists(),
            "missing yt-dlp sidecar at {}",
            ytdlp.display()
        );

        for part in [first_a, first_b] {
            let out_dir = root.join("未分类").join(&part.subdir);
            std::fs::create_dir_all(&out_dir).unwrap();
            let template = out_dir
                .join(format!("{}.%(ext)s", part.output_stem))
                .to_string_lossy()
                .into_owned();
            let status = Command::new(&ytdlp)
                .args([
                    "--newline",
                    "--no-playlist",
                    "-f",
                    "bv*[height<=720][vcodec^=avc1]+ba[acodec^=mp4a]/b[height<=720][vcodec^=avc1]/bv*[height<=720]+ba/b[height<=720]",
                    "--merge-output-format",
                    "mp4",
                    "-o",
                    &template,
                    "--no-mtime",
                    &part.url,
                ])
                .status()
                .expect("spawn yt-dlp");
            assert!(status.success(), "yt-dlp failed for {}", part.url);
            let matches: Vec<_> = std::fs::read_dir(&out_dir)
                .unwrap()
                .filter_map(|e| e.ok())
                .filter(|e| {
                    e.path()
                        .extension()
                        .and_then(|x| x.to_str())
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("mp4"))
                })
                .collect();
            assert!(
                !matches.is_empty(),
                "no mp4 in {}",
                out_dir.display()
            );
            eprintln!("downloaded {} -> {}", part.id, matches[0].path().display());
        }
    }
}
