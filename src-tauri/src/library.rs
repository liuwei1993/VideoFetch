use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const UNCATEGORIZED: &str = "未分类";

const VIDEO_EXTS: &[&str] = &["mp4", "webm", "mkv", "avi", "m4v", "mp3"];

#[derive(Debug, Clone, Serialize)]
pub struct VideoItem {
    pub name: String,
    pub path: String,
    pub size: u64,
}

pub fn ensure_library_root(root: &Path) -> Result<(), String> {
    fs::create_dir_all(root).map_err(|e| format!("create library root: {e}"))?;
    let unc = root.join(UNCATEGORIZED);
    if !unc.exists() {
        fs::create_dir_all(&unc).map_err(|e| format!("create {UNCATEGORIZED}: {e}"))?;
    }
    Ok(())
}

fn validate_category_name(name: &str) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("分类名不能为空".into());
    }
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err("分类名非法".into());
    }
    Ok(())
}

pub fn list_categories(root: &Path) -> Result<Vec<String>, String> {
    ensure_library_root(root)?;
    let mut cats = Vec::new();
    for entry in fs::read_dir(root).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let ft = entry.file_type().map_err(|e| e.to_string())?;
        if ft.is_dir() {
            cats.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    cats.sort();
    if let Some(pos) = cats.iter().position(|c| c == UNCATEGORIZED) {
        let unc = cats.remove(pos);
        cats.insert(0, unc);
    }
    Ok(cats)
}

pub fn create_category(root: &Path, name: &str) -> Result<(), String> {
    validate_category_name(name)?;
    ensure_library_root(root)?;
    let path = root.join(name.trim());
    if path.exists() {
        return Err(format!("分类已存在: {}", name.trim()));
    }
    fs::create_dir_all(&path).map_err(|e| e.to_string())
}

pub fn rename_category(root: &Path, from: &str, to: &str) -> Result<(), String> {
    validate_category_name(from)?;
    validate_category_name(to)?;
    if from.trim() == UNCATEGORIZED {
        return Err("不能重命名「未分类」".into());
    }
    let from_path = root.join(from.trim());
    let to_path = root.join(to.trim());
    if !from_path.exists() {
        return Err(format!("分类不存在: {}", from.trim()));
    }
    if to_path.exists() {
        return Err(format!("目标分类已存在: {}", to.trim()));
    }
    fs::rename(&from_path, &to_path).map_err(|e| e.to_string())
}

pub fn delete_category(root: &Path, name: &str, force: bool) -> Result<(), String> {
    validate_category_name(name)?;
    if name.trim() == UNCATEGORIZED {
        return Err("不能删除「未分类」".into());
    }
    let path = root.join(name.trim());
    if !path.exists() {
        return Err(format!("分类不存在: {}", name.trim()));
    }
    let empty = fs::read_dir(&path)
        .map_err(|e| e.to_string())?
        .next()
        .is_none();
    if !empty && !force {
        return Err("分类非空，请确认后强制删除".into());
    }
    if force {
        fs::remove_dir_all(&path).map_err(|e| e.to_string())
    } else {
        fs::remove_dir(&path).map_err(|e| e.to_string())
    }
}

fn is_video(path: &Path) -> bool {
    if path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("part"))
    {
        return false;
    }
    // Skip unfinished yt-dlp stream fragments like `title [id].f398.mp4`
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        if looks_like_ytdlp_fragment(name) {
            return false;
        }
    }
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| VIDEO_EXTS.iter().any(|x| x.eq_ignore_ascii_case(e)))
        .unwrap_or(false)
}

pub fn looks_like_ytdlp_fragment(name: &str) -> bool {
    // e.g. "... [aO-hLnBsVL4].f398.mp4" or "...f251.webm"
    let Some(stem) = Path::new(name).file_stem().and_then(|s| s.to_str()) else {
        return false;
    };
    stem.rsplit_once('.')
        .map(|(_, last)| {
            last.len() >= 2
                && last.as_bytes()[0].eq_ignore_ascii_case(&b'f')
                && last[1..].bytes().all(|b| b.is_ascii_digit())
        })
        .unwrap_or(false)
}

const TITLE_MAX_CHARS: usize = 25;

fn is_title_punctuation(c: char) -> bool {
    matches!(
        c,
        '！' | '？'
            | '。'
            | '，'
            | '、'
            | '；'
            | '：'
            | '…'
            | '!'
            | '?'
            | '.'
            | ','
            | ';'
            | ':'
            | '～'
            | '~'
            | '—'
            | '–'
    )
}

/// Keep at most 25 chars; if longer, cut at the last punctuation within those 25.
pub fn shorten_title(title: &str) -> String {
    let chars: Vec<char> = title.chars().collect();
    if chars.len() <= TITLE_MAX_CHARS {
        return title.to_string();
    }
    let window = &chars[..TITLE_MAX_CHARS];
    if let Some(pos) = window.iter().rposition(|c| is_title_punctuation(*c)) {
        return window[..=pos].iter().collect();
    }
    window.iter().collect()
}

/// Split `title [id]` stem into (title, id). Falls back to (stem, "") if no trailing `[id]`.
fn split_title_and_id(stem: &str) -> (String, Option<String>) {
    let Some(open) = stem.rfind(" [") else {
        return (stem.to_string(), None);
    };
    let rest = &stem[open + 2..];
    if rest.ends_with(']') && rest.len() > 1 {
        let id = rest[..rest.len() - 1].to_string();
        let title = stem[..open].to_string();
        return (title, Some(id));
    }
    (stem.to_string(), None)
}

/// Rename a finished download so the title part is shortened; keep `[id].ext`.
pub fn shorten_downloaded_filename(path: &Path) -> Result<PathBuf, String> {
    let parent = path
        .parent()
        .ok_or_else(|| "无效路径".to_string())?
        .to_path_buf();
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_string();
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| "无效文件名".to_string())?;

    let (title, id) = split_title_and_id(stem);
    let short = shorten_title(title.trim());
    if short == title.trim() {
        return Ok(path.to_path_buf());
    }

    let new_stem = match id {
        Some(id) => format!("{short} [{id}]"),
        None => short,
    };
    let new_name = if ext.is_empty() {
        new_stem
    } else {
        format!("{new_stem}.{ext}")
    };
    let dest = parent.join(&new_name);
    if dest == path {
        return Ok(path.to_path_buf());
    }
    if dest.exists() {
        return Err(format!("目标文件名已存在: {new_name}"));
    }
    fs::rename(path, &dest).map_err(|e| format!("重命名失败: {e}"))?;
    Ok(dest)
}

/// Shorten long titles for every video under the library root.
/// Conflicts / rename failures are skipped so the batch can continue.
pub fn shorten_all_titles(root: &Path) -> Result<usize, String> {
    ensure_library_root(root)?;
    let cats = list_categories(root)?;
    let mut renamed = 0;
    for cat in cats {
        let videos = list_videos(root, &cat)?;
        for v in videos {
            let path = PathBuf::from(&v.path);
            match shorten_downloaded_filename(&path) {
                Ok(new_path) if new_path != path => renamed += 1,
                Ok(_) => {}
                Err(e) => {
                    eprintln!("跳过缩短文件名 {}: {e}", v.name);
                }
            }
        }
    }
    Ok(renamed)
}

pub fn list_videos(root: &Path, category: &str) -> Result<Vec<VideoItem>, String> {
    validate_category_name(category)?;
    let dir = root.join(category.trim());
    if !dir.exists() {
        return Ok(vec![]);
    }
    let mut items = Vec::new();
    fn walk(dir: &Path, items: &mut Vec<VideoItem>) -> Result<(), String> {
        for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            if path.is_dir() {
                walk(&path, items)?;
            } else if path.is_file() && is_video(&path) {
                let meta = entry.metadata().map_err(|e| e.to_string())?;
                items.push(VideoItem {
                    name: entry.file_name().to_string_lossy().into_owned(),
                    path: path.to_string_lossy().into_owned(),
                    size: meta.len(),
                });
            }
        }
        Ok(())
    }
    walk(&dir, &mut items)?;
    items.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(items)
}

pub fn move_video(root: &Path, from_cat: &str, to_cat: &str, filename: &str) -> Result<(), String> {
    validate_category_name(from_cat)?;
    validate_category_name(to_cat)?;
    if filename.contains('/') || filename.contains('\\') || filename.contains("..") {
        return Err("文件名非法".into());
    }
    let to_dir = root.join(to_cat.trim());
    fs::create_dir_all(&to_dir).map_err(|e| e.to_string())?;
    let from = root.join(from_cat.trim()).join(filename);
    let to = to_dir.join(filename);
    if !from.exists() {
        return Err("视频不存在".into());
    }
    if to.exists() {
        return Err("目标已存在同名文件".into());
    }
    fs::rename(&from, &to).map_err(|e| e.to_string())
}

pub fn delete_video(root: &Path, category: &str, filename: &str) -> Result<(), String> {
    validate_category_name(category)?;
    if filename.contains('/') || filename.contains('\\') || filename.contains("..") {
        return Err("文件名非法".into());
    }
    let path = root.join(category.trim()).join(filename);
    if !path.exists() {
        return Err("视频不存在".into());
    }
    fs::remove_file(&path).map_err(|e| e.to_string())
}

pub fn open_video(path: &Path) -> Result<(), String> {
    let path = PathBuf::from(path);
    if !path.exists() {
        return Err("文件不存在".into());
    }
    #[cfg(target_os = "linux")]
    {
        Command::new("xdg-open")
            .arg(&path)
            .spawn()
            .map_err(|e| format!("打开失败: {e}"))?;
    }
    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(&path)
            .spawn()
            .map_err(|e| format!("打开失败: {e}"))?;
    }
    #[cfg(target_os = "windows")]
    {
        Command::new("cmd")
            .args(["/C", "start", ""])
            .arg(&path)
            .spawn()
            .map_err(|e| format!("打开失败: {e}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn create_and_list_category() {
        let dir = tempfile::tempdir().unwrap();
        ensure_library_root(dir.path()).unwrap();
        create_category(dir.path(), "脱口秀").unwrap();
        let cats = list_categories(dir.path()).unwrap();
        assert!(cats.contains(&"未分类".into()));
        assert!(cats.contains(&"脱口秀".into()));
        assert_eq!(cats[0], "未分类");
    }

    #[test]
    fn list_videos_recurses_into_subdirs() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let nested = root.join("未分类").join("AI入门").join("课1");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("a [BV1_p1].mp4"), b"x").unwrap();
        std::fs::write(root.join("未分类").join("top.mp4"), b"y").unwrap();
        let items = list_videos(root, "未分类").unwrap();
        assert_eq!(items.len(), 2);
        assert!(items.iter().any(|v| v.name == "a [BV1_p1].mp4"));
        assert!(items.iter().any(|v| v.name == "top.mp4"));
    }

    #[test]
    fn move_video_between_categories() {
        let dir = tempfile::tempdir().unwrap();
        ensure_library_root(dir.path()).unwrap();
        create_category(dir.path(), "A").unwrap();
        create_category(dir.path(), "B").unwrap();
        let file = dir.path().join("A").join("clip.mp4");
        let mut f = fs::File::create(&file).unwrap();
        writeln!(f, "x").unwrap();
        move_video(dir.path(), "A", "B", "clip.mp4").unwrap();
        assert!(!file.exists());
        assert!(dir.path().join("B").join("clip.mp4").exists());
        let videos = list_videos(dir.path(), "B").unwrap();
        assert_eq!(videos.len(), 1);
        assert_eq!(videos[0].name, "clip.mp4");
    }

    #[test]
    fn shorten_title_cuts_at_last_punct_within_25() {
        let title = "毛豆脱6全面进化🤯全程炸场根本无解！总决赛超细腻文本依旧炸翻！开口直接笑麻了！ #脱口秀 #脱口秀大会 #脱口秀和ta的朋友们 #毛豆";
        assert_eq!(shorten_title(title), "毛豆脱6全面进化🤯全程炸场根本无解！");
    }

    #[test]
    fn shorten_title_hard_cut_without_punct() {
        let title = "一二三四五六七八九十甲乙丙丁戊己庚辛壬癸子丑寅卯辰巳午";
        assert_eq!(title.chars().count(), 27);
        assert_eq!(
            shorten_title(title),
            "一二三四五六七八九十甲乙丙丁戊己庚辛壬癸子丑寅卯辰"
        );
    }

    #[test]
    fn shorten_all_titles_renames_library_files() {
        let dir = tempfile::tempdir().unwrap();
        ensure_library_root(dir.path()).unwrap();
        create_category(dir.path(), "脱口秀").unwrap();
        let long = "毛豆脱6全面进化🤯全程炸场根本无解！总决赛超细腻文本依旧炸翻！开口直接笑麻了！ [aO-hLnBsVL4].mp4";
        let path = dir.path().join("脱口秀").join(long);
        let mut f = fs::File::create(&path).unwrap();
        writeln!(f, "x").unwrap();

        let n = shorten_all_titles(dir.path()).unwrap();
        assert_eq!(n, 1);
        assert!(!path.exists());
        let expected = dir
            .path()
            .join("脱口秀")
            .join("毛豆脱6全面进化🤯全程炸场根本无解！ [aO-hLnBsVL4].mp4");
        assert!(expected.exists());
    }
}
