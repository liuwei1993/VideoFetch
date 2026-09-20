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

pub fn format_selector(quality: &str) -> String {
    match quality {
        "1080" => "bv*[height<=1080]+ba/b[height<=1080]".into(),
        "best" => "bv*+ba/b".into(),
        _ => "bv*[height<=720]+ba/b[height<=720]".into(),
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
    fn format_and_proxy() {
        assert!(format_selector("720").contains("720"));
        let mut s = Settings::default();
        assert!(proxy_for(Site::Youtube, &s).is_some());
        assert!(proxy_for(Site::Bilibili, &s).is_none());
        s.bilibili_use_proxy = true;
        assert!(proxy_for(Site::Bilibili, &s).is_some());
    }
}
