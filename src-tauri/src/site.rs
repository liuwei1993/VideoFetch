use crate::settings::Settings;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Site {
    Youtube,
    Bilibili,
    Unknown,
}

pub fn detect_site(url: &str) -> Site {
    let lower = url.to_lowercase();
    if lower.contains("youtube.com") || lower.contains("youtu.be") {
        Site::Youtube
    } else if lower.contains("bilibili.com") || lower.contains("b23.tv") {
        Site::Bilibili
    } else {
        Site::Unknown
    }
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
        Site::Youtube => Some(proxy.to_string()),
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
    fn format_and_proxy() {
        let sel = format_selector("720");
        assert!(sel.contains("720"));
        assert!(sel.contains("avc1"));
        assert!(sel.contains("mp4a"));
        let mut s = Settings::default();
        assert!(proxy_for(Site::Youtube, &s).is_some());
        assert!(proxy_for(Site::Bilibili, &s).is_none());
        s.bilibili_use_proxy = true;
        assert!(proxy_for(Site::Bilibili, &s).is_some());
    }
}
