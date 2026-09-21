use crate::settings::Settings;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Site {
    Youtube,
    Bilibili,
    Missav,
    Unknown,
}

const MISSAV_LANGS: &[&str] = &[
    "cn", "en", "ja", "ko", "ms", "th", "de", "fr", "vi", "id", "fil", "pt",
];
const MISSAV_LISTING_SLUGS: &[&str] = &[
    "makers",
    "actresses",
    "genres",
    "articles",
    "ads",
    "history",
    "contact",
    "chinese-subtitle",
    "ranking",
    "fc2",
];

pub fn detect_site(url: &str) -> Site {
    let lower = url.to_lowercase();
    if lower.contains("youtube.com") || lower.contains("youtu.be") {
        Site::Youtube
    } else if lower.contains("bilibili.com") || lower.contains("b23.tv") {
        Site::Bilibili
    } else if is_missav_host(&lower) {
        Site::Missav
    } else {
        Site::Unknown
    }
}

fn url_host(url: &str) -> &str {
    let rest = match url.find("://") {
        Some(i) => &url[i + 3..],
        None => url,
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    host.strip_prefix("www.").unwrap_or(host)
}

fn is_missav_host(url_lower: &str) -> bool {
    matches!(
        url_host(url_lower),
        "missav.ws" | "missav.com" | "missav.ai"
    )
}

/// Single watch pages such as `/cn/clot-044`. Homepage (`/dm247/cn`) and
/// listing pages are not treated as a downloadable video in this phase.
pub fn is_missav_single_video_url(url: &str) -> bool {
    if detect_site(url) != Site::Missav {
        return false;
    }
    let lower = url.to_lowercase();
    let without_fragment = lower.split('#').next().unwrap_or(lower.as_str());
    let path_raw = without_fragment
        .split('?')
        .next()
        .unwrap_or(without_fragment);
    let segs: Vec<&str> = url_path(path_raw)
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();
    let id = match segs.as_slice() {
        [id] => *id,
        [lang, id] if MISSAV_LANGS.contains(lang) => *id,
        _ => return false,
    };
    is_missav_video_id(id)
}

fn is_missav_video_id(id: &str) -> bool {
    if id.len() > 2 && id.starts_with("dm") && id[2..].bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    if MISSAV_LISTING_SLUGS.contains(&id) {
        return false;
    }
    id.contains('-') || id.bytes().any(|b| b.is_ascii_digit())
}

/// Bilibili space season/series collection pages (not single BV pages).
pub fn is_bilibili_collection_url(url: &str) -> bool {
    let lower = url.to_lowercase();
    if !(lower.contains("bilibili.com") || lower.contains("b23.tv")) {
        return false;
    }
    // space.bilibili.com/<mid>/lists/<id>
    lower.contains("space.bilibili.com") && lower.contains("/lists/")
}

/// YouTube channel tabs / playlists that should expand as a batch (not a single watch URL).
pub fn is_youtube_collection_url(url: &str) -> bool {
    let lower = url.to_lowercase();
    if !lower.contains("youtube.com") {
        return false;
    }
    if lower.contains("youtu.be/") {
        return false;
    }
    if lower.contains("/watch") {
        return false;
    }

    let without_fragment = lower.split('#').next().unwrap_or(lower.as_str());
    let (path_raw, query) = match without_fragment.split_once('?') {
        Some((path, query)) => (path, query),
        None => (without_fragment, ""),
    };
    let path = url_path(path_raw).trim_end_matches('/');

    if path.ends_with("/playlists") || path.ends_with("/community") || path.ends_with("/about") {
        return false;
    }
    if path.ends_with("/playlist") && query.contains("list=") {
        return true;
    }
    if path.ends_with("/videos") || path.ends_with("/shorts") || path.ends_with("/streams") {
        return true;
    }

    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    segments.len() == 1 && segments[0].starts_with('@') && segments[0].len() > 1
}

pub fn is_batch_url(url: &str) -> bool {
    is_bilibili_collection_url(url) || is_youtube_collection_url(url)
}

fn url_path(url_without_query: &str) -> &str {
    let rest = match url_without_query.find("://") {
        Some(i) => &url_without_query[i + 3..],
        None => url_without_query,
    };
    match rest.find('/') {
        Some(i) => &rest[i..],
        None => "/",
    }
}

/// Prefer H.264 (avc1) + AAC (mp4a) so outputs play on phones (many Huawei /
/// HarmonyOS players lack AV1/VP9). Fall back to best available if needed.
pub fn format_selector(quality: &str) -> String {
    match quality {
        "1080" => {
            "bv*[height<=1080][vcodec^=avc1]+ba[acodec^=mp4a]/b[height<=1080][vcodec^=avc1]/bv*[height<=1080]+ba/b[height<=1080]"
                .into()
        }
        "best" => "bv*[vcodec^=avc1]+ba[acodec^=mp4a]/b[vcodec^=avc1]/bv*+ba/b".into(),
        _ => {
            "bv*[height<=720][vcodec^=avc1]+ba[acodec^=mp4a]/b[height<=720][vcodec^=avc1]/bv*[height<=720]+ba/b[height<=720]"
                .into()
        }
    }
}

pub fn proxy_for(site: Site, settings: &Settings) -> Option<String> {
    let proxy = settings.youtube_proxy.trim();
    if proxy.is_empty() {
        return None;
    }
    match site {
        Site::Youtube | Site::Missav => Some(proxy.to_string()),
        Site::Bilibili if settings.bilibili_use_proxy => Some(proxy.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_youtube_and_bilibili() {
        assert_eq!(
            detect_site("https://www.youtube.com/watch?v=abc"),
            Site::Youtube
        );
        assert_eq!(detect_site("https://youtu.be/abc"), Site::Youtube);
        assert_eq!(
            detect_site("https://www.bilibili.com/video/BV1xx"),
            Site::Bilibili
        );
        assert_eq!(detect_site("https://b23.tv/xyz"), Site::Bilibili);
    }

    #[test]
    fn detect_bilibili_collection_urls() {
        assert!(is_bilibili_collection_url(
            "https://space.bilibili.com/7504289/lists/6254946?type=season"
        ));
        assert!(is_bilibili_collection_url(
            "https://space.bilibili.com/7504289/lists/6254946?type=series"
        ));
        assert!(is_bilibili_collection_url(
            "https://SPACE.BILIBILI.COM/1/lists/2"
        ));
        assert!(!is_bilibili_collection_url(
            "https://www.bilibili.com/video/BV1xx411c7mD"
        ));
        assert!(!is_bilibili_collection_url("https://b23.tv/abcdef"));
        assert!(!is_bilibili_collection_url(
            "https://www.youtube.com/playlist?list=PLxxx"
        ));
    }

    #[test]
    fn detect_youtube_collection_urls() {
        assert!(is_youtube_collection_url(
            "https://www.youtube.com/@bruce_lu_1993/videos"
        ));
        assert!(is_youtube_collection_url(
            "https://www.youtube.com/@bruce_lu_1993"
        ));
        assert!(is_youtube_collection_url(
            "https://www.youtube.com/@bruce_lu_1993/"
        ));
        assert!(is_youtube_collection_url(
            "https://m.youtube.com/@bruce_lu_1993/shorts"
        ));
        assert!(is_youtube_collection_url(
            "https://www.youtube.com/@x/streams?filter=archive"
        ));
        assert!(is_youtube_collection_url(
            "https://www.youtube.com/playlist?list=PLxxx"
        ));
        assert!(is_youtube_collection_url("https://YOUTUBE.COM/@Foo/Videos"));
        assert!(is_youtube_collection_url(
            "https://www.youtube.com/channel/UCxxxx/videos"
        ));

        assert!(!is_youtube_collection_url(
            "https://www.youtube.com/watch?v=abc"
        ));
        assert!(!is_youtube_collection_url(
            "https://www.youtube.com/watch?v=abc&list=PLxxx"
        ));
        assert!(!is_youtube_collection_url("https://youtu.be/abc"));
        assert!(!is_youtube_collection_url(
            "https://www.youtube.com/@x/playlists"
        ));
        assert!(!is_youtube_collection_url(
            "https://www.youtube.com/@x/community"
        ));
        assert!(!is_youtube_collection_url(
            "https://www.youtube.com/@x/about"
        ));
        assert!(!is_youtube_collection_url(
            "https://www.bilibili.com/video/BV1xx"
        ));
        assert!(!is_youtube_collection_url(
            "https://space.bilibili.com/1/lists/2"
        ));
    }

    #[test]
    fn detect_batch_urls() {
        assert!(is_batch_url(
            "https://space.bilibili.com/7504289/lists/6254946?type=season"
        ));
        assert!(is_batch_url(
            "https://www.youtube.com/@bruce_lu_1993/videos"
        ));
        assert!(!is_batch_url("https://www.youtube.com/watch?v=abc"));
        assert!(!is_batch_url("https://www.bilibili.com/video/BV1xx411c7mD"));
        assert!(!is_batch_url("https://missav.ws/dm247/cn"));
        assert!(!is_batch_url("https://missav.ws/cn/clot-044"));
    }

    #[test]
    fn detect_missav_hosts() {
        assert_eq!(detect_site("https://missav.ws/cn/clot-044"), Site::Missav);
        assert_eq!(
            detect_site("https://www.missav.com/en/blk-470"),
            Site::Missav
        );
        assert_eq!(detect_site("https://missav.ai/dm247/cn"), Site::Missav);
        assert_eq!(
            detect_site("https://example.com/missav.ws/x"),
            Site::Unknown
        );
    }

    #[test]
    fn missav_single_video_vs_homepage() {
        assert!(is_missav_single_video_url("https://missav.ws/cn/clot-044"));
        assert!(is_missav_single_video_url(
            "https://missav.ai/en/blk-470-uncensored-leak?foo=1"
        ));
        assert!(is_missav_single_video_url(
            "https://missav.com/fc2-ppv-4975133"
        ));
        assert!(!is_missav_single_video_url("https://missav.ws/dm247/cn"));
        assert!(!is_missav_single_video_url("https://missav.ai/dm247"));
        assert!(!is_missav_single_video_url("https://missav.ai/cn/makers"));
        assert!(!is_missav_single_video_url("https://missav.ai/cn/fc2"));
        assert!(!is_missav_single_video_url(
            "https://www.youtube.com/watch?v=abc"
        ));
    }

    #[test]
    fn format_and_proxy() {
        let sel = format_selector("720");
        assert!(sel.contains("720"));
        assert!(sel.contains("avc1"));
        assert!(sel.contains("mp4a"));
        let mut s = Settings::default();
        assert!(proxy_for(Site::Youtube, &s).is_some());
        assert!(proxy_for(Site::Missav, &s).is_some());
        assert_eq!(proxy_for(Site::Missav, &s), proxy_for(Site::Youtube, &s));
        assert!(proxy_for(Site::Bilibili, &s).is_none());
        s.bilibili_use_proxy = true;
        assert!(proxy_for(Site::Bilibili, &s).is_some());
        s.youtube_proxy = String::new();
        assert!(proxy_for(Site::Missav, &s).is_none());
    }
}
