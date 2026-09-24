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
}
