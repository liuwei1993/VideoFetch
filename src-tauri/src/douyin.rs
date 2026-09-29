use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub const MOBILE_UA: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 16_6 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/16.6 Mobile/15E148 Safari/604.1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedDouyin {
    pub aweme_id: String,
    pub title: String,
    pub video_id: String,
    pub ratio: String,
    pub play_url: String,
}

impl ResolvedDouyin {
    pub fn output_stem(&self) -> String {
        let title = crate::bilibili::sanitize_path_component(&self.title);
        format!("{title} [{}]", self.aweme_id)
    }
}

/// `1080` / `best` → 1080p, everything else (including the default 720) → 720p.
pub fn ratio_for_quality(quality: &str) -> &'static str {
    match quality {
        "1080" | "best" => "1080p",
        _ => "720p",
    }
}

pub fn play_url(video_id: &str, quality: &str) -> String {
    let ratio = ratio_for_quality(quality);
    format!("https://aweme.snssdk.com/aweme/v1/play/?video_id={video_id}&ratio={ratio}&line=0")
}

/// Numeric post id from a Douyin / iesdouyin URL, including one buried in share text.
pub fn extract_aweme_id(input: &str) -> Option<String> {
    let url = first_http_url(input).unwrap_or_else(|| input.trim().to_string());
    if let Some(id) = query_param(&url, "modal_id") {
        if is_aweme_id(&id) {
            return Some(id);
        }
    }
    let path = url_path(&url);
    for marker in [
        "/share/video/",
        "/share/note/",
        "/share/slides/",
        "/video/",
        "/note/",
        "/slides/",
    ] {
        if let Some(rest) = path_after(path, marker) {
            let id: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            if is_aweme_id(&id) {
                return Some(id);
            }
        }
    }
    None
}

/// Page Chrome should open. Known ids become a canonical watch URL; short links stay as-is.
pub fn watch_page_url(input: &str) -> String {
    let raw = first_http_url(input).unwrap_or_else(|| input.trim().to_string());
    if let Some(id) = extract_aweme_id(&raw) {
        return format!("https://www.douyin.com/video/{id}");
    }
    if raw.starts_with("http://") || raw.starts_with("https://") {
        return raw;
    }
    let lower = raw.to_lowercase();
    if lower.contains("douyin.com") || lower.contains("iesdouyin.com") {
        return format!("https://{raw}");
    }
    raw
}

pub fn resolve(
    input: &str,
    quality: &str,
    cancelled: Option<&AtomicBool>,
) -> Result<ResolvedDouyin, String> {
    let page = watch_page_url(input);
    let fallback_id = extract_aweme_id(input);
    // First dump; if the player payload is still missing, retry once with a longer budget.
    let html = dump_watch_page(&page, cancelled, 20_000)?;
    match parse_watch_html(&html, fallback_id.as_deref(), quality) {
        Ok(media) => Ok(media),
        Err(first_err) => {
            if cancelled_now(cancelled) {
                return Err("已停止下载".into());
            }
            let html2 = dump_watch_page(&page, cancelled, 35_000)?;
            parse_watch_html(&html2, fallback_id.as_deref(), quality).map_err(|_| first_err)
        }
    }
}

pub fn parse_watch_html(
    html: &str,
    fallback_aweme_id: Option<&str>,
    quality: &str,
) -> Result<ResolvedDouyin, String> {
    let html = html.replace("&amp;", "&").replace("\\u0026", "&");
    let from_url = fallback_aweme_id.filter(|id| is_aweme_id(id));
    let from_page = canonical_aweme_id(&html);
    let aweme_id = match (from_url, from_page.as_deref()) {
        (Some(url_id), Some(page_id)) if url_id != page_id => {
            return Err(format!(
                "抖音作品 id 不一致（链接 {url_id} / 页面 {page_id}），已中止以免下错视频"
            ));
        }
        (Some(url_id), _) => url_id.to_string(),
        (None, Some(page_id)) => page_id.to_string(),
        (None, None) => return Err("没能从抖音页面解析出作品 id".into()),
    };
    let video_id = video_id_for_aweme(&html, &aweme_id).ok_or_else(|| {
        "没能从抖音页面解析出可播视频（可能是图文/笔记，或页面尚未加载完成）".to_string()
    })?;
    let mut title = extract_title(&html);
    if title.is_empty() {
        title = aweme_id.clone();
    }
    let ratio = ratio_for_quality(quality).to_string();
    let play_url = play_url(&video_id, quality);
    Ok(ResolvedDouyin {
        aweme_id,
        title,
        video_id,
        ratio,
        play_url,
    })
}

fn dump_watch_page(
    url: &str,
    cancelled: Option<&AtomicBool>,
    virtual_time_ms: u64,
) -> Result<String, String> {
    if cancelled_now(cancelled) {
        return Err("已停止下载".into());
    }
    let bin = chrome_bin()?;
    let dir = std::env::temp_dir().join(format!(
        "videofetch-douyin-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0),
        fastrand_u32()
    ));
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建临时目录失败: {e}"))?;
    let _guard = TempDir(dir.clone());

    let page_file = dir.join("page.html");
    let stdout_file =
        std::fs::File::create(&page_file).map_err(|e| format!("创建抖音页面输出失败: {e}"))?;
    let mut cmd = Command::new(&bin);
    cmd.arg("--headless=new")
        .arg("--disable-gpu")
        .arg("--disable-dev-shm-usage")
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .arg("--disable-extensions")
        .arg("--disable-background-networking")
        .arg("--mute-audio")
        .arg(format!("--virtual-time-budget={virtual_time_ms}"))
        .arg("--timeout=25000")
        .arg(format!("--user-data-dir={}", dir.display()))
        .arg("--dump-dom")
        .arg(url)
        .stdout(Stdio::from(stdout_file))
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("启动 Chrome 解析抖音失败: {e}"))?;
    let started = Instant::now();
    let status = loop {
        if cancelled_now(cancelled) {
            kill_group(&mut child);
            return Err("已停止下载".into());
        }
        if started.elapsed() > Duration::from_secs(45) {
            kill_group(&mut child);
            return Err("解析抖音超时。请确认本机能打开 douyin.com 后重试。".into());
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => std::thread::sleep(Duration::from_millis(200)),
            Err(e) => return Err(format!("等待 Chrome 失败: {e}")),
        }
    };
    let html = std::fs::read_to_string(&page_file).map_err(|e| format!("读取抖音页面失败: {e}"))?;
    if html.len() < 1000 {
        return Err(format!(
            "抖音页面内容为空（Chrome 退出码 {:?}）",
            status.code()
        ));
    }
    let host = final_page_host(&html).unwrap_or_default();
    if !host.is_empty()
        && !(host == "douyin.com"
            || host.ends_with(".douyin.com")
            || host == "iesdouyin.com"
            || host.ends_with(".iesdouyin.com"))
    {
        return Err(format!("抖音短链跳转到了非抖音站点（{host}），已中止"));
    }
    Ok(html)
}

fn cancelled_now(cancelled: Option<&AtomicBool>) -> bool {
    cancelled.map(|c| c.load(Ordering::SeqCst)).unwrap_or(false)
}

fn fastrand_u32() -> u32 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    Instant::now().hash(&mut h);
    std::process::id().hash(&mut h);
    (h.finish() as u32)
        .wrapping_mul(1664525)
        .wrapping_add(1013904223)
}

fn chrome_bin() -> Result<String, String> {
    for name in [
        "google-chrome",
        "google-chrome-stable",
        "chromium",
        "chromium-browser",
        "chrome",
    ] {
        if command_exists(name) {
            return Ok(name.to_string());
        }
    }
    for path in [
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Chromium.app/Contents/MacOS/Chromium",
        "/usr/bin/google-chrome",
        "/usr/bin/chromium",
        "/usr/bin/chromium-browser",
    ] {
        if PathBuf::from(path).is_file() {
            return Ok(path.to_string());
        }
    }
    Err("解析抖音需要本机的 Chrome 或 Chromium（未找到 google-chrome / chromium）".into())
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

fn kill_group(child: &mut std::process::Child) {
    let pid = child.id();
    #[cfg(unix)]
    {
        let _ = Command::new("kill")
            .args(["-KILL", &format!("-{pid}")])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

struct TempDir(PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn is_aweme_id(id: &str) -> bool {
    id.len() >= 8 && id.chars().all(|c| c.is_ascii_digit())
}

fn first_http_url(text: &str) -> Option<String> {
    let lower = text.to_lowercase();
    let start = lower.find("https://").or_else(|| lower.find("http://"))?;
    let rest = &text[start..];
    let end = rest.find(|c: char| c.is_whitespace()).unwrap_or(rest.len());
    let url = rest[..end].trim_end_matches(|c: char| {
        matches!(
            c,
            '，' | '。' | '！' | '!' | '）' | ')' | ']' | '"' | '\'' | '>'
        )
    });
    if url.len() < 12 {
        None
    } else {
        Some(url.to_string())
    }
}

fn query_param(url: &str, key: &str) -> Option<String> {
    let query = url.split('?').nth(1)?.split('#').next()?;
    let prefix = format!("{key}=");
    for part in query.split('&') {
        if let Some(value) = part.strip_prefix(&prefix) {
            return Some(value.to_string());
        }
    }
    None
}

fn url_path(url: &str) -> &str {
    let rest = match url.find("://") {
        Some(i) => &url[i + 3..],
        None => url,
    };
    let rest = rest.split(['?', '#']).next().unwrap_or(rest);
    match rest.find('/') {
        Some(i) => &rest[i..],
        None => "/",
    }
}

fn path_after<'a>(path: &'a str, marker: &str) -> Option<&'a str> {
    let lower = path.to_lowercase();
    let idx = lower.find(marker)?;
    Some(&path[idx + marker.len()..])
}

fn canonical_aweme_id(html: &str) -> Option<String> {
    let key = "rel=\"canonical\" href=\"";
    let i = html.find(key)?;
    let rest = &html[i + key.len()..];
    let end = rest.find('"')?;
    extract_aweme_id(&rest[..end])
}

fn final_page_host(html: &str) -> Option<String> {
    let key = "rel=\"canonical\" href=\"";
    let i = html.find(key)?;
    let rest = &html[i + key.len()..];
    let end = rest.find('"')?;
    let href = &rest[..end];
    let after = href.find("://").map(|n| &href[n + 3..]).unwrap_or(href);
    let host = after.split(['/', '?', '#']).next().unwrap_or(after);
    let host = host.strip_prefix("www.").unwrap_or(host);
    if host.is_empty() {
        None
    } else {
        Some(host.to_lowercase())
    }
}

fn extract_title(html: &str) -> String {
    let start = match html.find("<title>") {
        Some(i) => i + "<title>".len(),
        None => return String::new(),
    };
    let rest = &html[start..];
    let end = rest.find("</title>").unwrap_or(rest.len().min(300));
    let raw = rest[..end]
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'");
    let collapsed = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.trim_end_matches(" - 抖音").trim().to_string()
}

/// Only accept a `video_id` that appears near this aweme id — never the first hit on the page.
fn video_id_for_aweme(html: &str, aweme_id: &str) -> Option<String> {
    let needle = format!("__vid={aweme_id}");
    if let Some(pos) = html.find(&needle) {
        let start = pos.saturating_sub(1200);
        let end = (pos + needle.len() + 200).min(html.len());
        if let Some(id) = find_video_id_in(&html[start..end]) {
            return Some(id);
        }
    }

    const KEY: &str = "video_id=";
    let mut from = 0;
    while let Some(rel) = html[from..].find(KEY) {
        let i = from + rel;
        let after = i + KEY.len();
        let id: String = html[after..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect();
        if id.len() >= 16 && id.starts_with('v') {
            let window_start = i.saturating_sub(400);
            let window_end = (after + id.len() + 400).min(html.len());
            if html[window_start..window_end].contains(aweme_id) {
                return Some(id);
            }
        }
        from = after;
    }
    None
}

fn find_video_id_in(s: &str) -> Option<String> {
    const KEY: &str = "video_id=";
    if let Some(i) = s.find(KEY) {
        let id: String = s[i + KEY.len()..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect();
        if id.len() >= 16 && id.starts_with('v') {
            return Some(id);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_aweme_id_from_common_links() {
        assert_eq!(
            extract_aweme_id(
                "https://www.iesdouyin.com/share/video/7289364577651821876/?region=CN&from_ssr=1"
            ),
            Some("7289364577651821876".into())
        );
        assert_eq!(
            extract_aweme_id("https://www.douyin.com/video/7289364577651821876"),
            Some("7289364577651821876".into())
        );
        assert_eq!(
            extract_aweme_id("https://www.douyin.com/note/7289364577651821876/"),
            Some("7289364577651821876".into())
        );
        assert_eq!(
            extract_aweme_id(
                "https://www.douyin.com/user/MS4wLjABAAAA?modal_id=7289364577651821876"
            ),
            Some("7289364577651821876".into())
        );
        assert_eq!(
            extract_aweme_id("https://www.douyin.com/jingxuan/food?modal_id=7663064252127611370"),
            Some("7663064252127611370".into())
        );
        assert_eq!(
            watch_page_url("https://www.douyin.com/jingxuan/food?modal_id=7663064252127611370"),
            "https://www.douyin.com/video/7663064252127611370"
        );
        assert_eq!(
            extract_aweme_id(
                "7.64 复制打开抖音 https://www.douyin.com/video/7289364577651821876 复制此链接"
            ),
            Some("7289364577651821876".into())
        );
        assert_eq!(extract_aweme_id("https://v.douyin.com/iR2syBRn/"), None);
        assert_eq!(
            extract_aweme_id("https://www.youtube.com/watch?v=abc"),
            None
        );
    }

    #[test]
    fn watch_page_normalizes_share_links() {
        assert_eq!(
            watch_page_url("https://www.iesdouyin.com/share/video/7289364577651821876/?region=CN"),
            "https://www.douyin.com/video/7289364577651821876"
        );
        assert_eq!(
            watch_page_url("https://v.douyin.com/iR2syBRn/"),
            "https://v.douyin.com/iR2syBRn/"
        );
        assert_eq!(
            watch_page_url("v.douyin.com/iR2syBRn/"),
            "https://v.douyin.com/iR2syBRn/"
        );
    }

    #[test]
    fn parses_watch_html_into_play_url() {
        let html = r#"<title>你好世界
 - 抖音</title><link rel="canonical" href="https://www.douyin.com/video/7289364577651821876"><video src="https://example.com/a?video_id=v0d00fg10000ckkfukjc77ufj44h2irg&amp;__vid=7289364577651821876">"#;
        let media = parse_watch_html(html, Some("7289364577651821876"), "1080").unwrap();
        assert_eq!(media.aweme_id, "7289364577651821876");
        assert_eq!(media.title, "你好世界");
        assert_eq!(media.video_id, "v0d00fg10000ckkfukjc77ufj44h2irg");
        assert_eq!(media.ratio, "1080p");
        assert_eq!(
            media.play_url,
            "https://aweme.snssdk.com/aweme/v1/play/?video_id=v0d00fg10000ckkfukjc77ufj44h2irg&ratio=1080p&line=0"
        );
        assert_eq!(ratio_for_quality("720"), "720p");
        assert_eq!(ratio_for_quality("best"), "1080p");
        assert!(media.output_stem().contains("[7289364577651821876]"));
    }

    #[test]
    fn rejects_unrelated_recommend_video_id() {
        let padding = "x".repeat(2000);
        let html = format!(
            r#"<link rel="canonical" href="https://www.douyin.com/video/1111111111111111111"><title>目标 - 抖音</title>
{padding}
<a href="https://cdn/x?video_id=v0aaaaaaaaaaaaaaaaaaaa&other=1">recommend</a>
"#
        );
        let err = parse_watch_html(&html, Some("1111111111111111111"), "720").unwrap_err();
        assert!(err.contains("可播视频"), "{err}");
    }

    #[test]
    fn accepts_video_id_near_aweme_without_vid_param() {
        let html = r#"<link rel="canonical" href="https://www.douyin.com/video/7663064252127611370"><title>午餐 - 抖音</title>
<script>play?video_id=v0200fg10000d9cai727dld4bbsjifl0&aid=6383&target=7663064252127611370</script>"#;
        let media = parse_watch_html(html, Some("7663064252127611370"), "720").unwrap();
        assert_eq!(media.video_id, "v0200fg10000d9cai727dld4bbsjifl0");
    }

    #[test]
    fn rejects_mismatched_url_and_canonical_ids() {
        let html = r#"<link rel="canonical" href="https://www.douyin.com/video/2222222222222222222"><title>x - 抖音</title>
<video src="https://x?video_id=v0bbbbbbbbbbbbbbbbbbbb&__vid=2222222222222222222">"#;
        let err = parse_watch_html(html, Some("1111111111111111111"), "720").unwrap_err();
        assert!(err.contains("不一致"), "{err}");
    }

    #[test]
    #[ignore = "needs Chrome and network"]
    fn resolve_live_share_url() {
        let media = resolve(
            "https://www.iesdouyin.com/share/video/7289364577651821876/?region=CN",
            "720",
            None,
        )
        .unwrap();
        assert_eq!(media.aweme_id, "7289364577651821876");
        assert!(media.video_id.starts_with('v'));
        assert!(media.play_url.contains("ratio=720p"));
        assert!(!media.title.is_empty());
    }
}
