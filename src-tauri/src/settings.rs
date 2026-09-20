use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub library_root: String,
    pub default_quality: String,
    pub last_category: String,
    pub youtube_proxy: String,
    pub bilibili_use_proxy: bool,
    pub cookie_file: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            library_root: "~/videofetch".into(),
            default_quality: "720".into(),
            last_category: "未分类".into(),
            youtube_proxy: "http://127.0.0.1:57890".into(),
            bilibili_use_proxy: false,
            cookie_file: None,
        }
    }
}

pub fn expand_path(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = home_dir() {
            return home.join(rest);
        }
    } else if path == "~" {
        if let Some(home) = home_dir() {
            return home;
        }
    }
    PathBuf::from(path)
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

pub fn settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("config dir: {e}"))?;
    Ok(dir.join("settings.json"))
}

pub fn load_settings(app: &AppHandle) -> Result<Settings, String> {
    let path = settings_path(app)?;
    if !path.exists() {
        let s = Settings::default();
        save_settings_to(&path, &s)?;
        return Ok(s);
    }
    let raw = fs::read_to_string(&path).map_err(|e| format!("read settings: {e}"))?;
    serde_json::from_str(&raw).map_err(|e| format!("parse settings: {e}"))
}

pub fn save_settings(app: &AppHandle, settings: &Settings) -> Result<(), String> {
    let path = settings_path(app)?;
    save_settings_to(&path, settings)
}

fn save_settings_to(path: &Path, settings: &Settings) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create config dir: {e}"))?;
    }
    let raw = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    fs::write(path, raw).map_err(|e| format!("write settings: {e}"))
}

pub fn library_root_path(settings: &Settings) -> PathBuf {
    expand_path(&settings.library_root)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_tilde() {
        let p = expand_path("~/videofetch");
        assert!(p.is_absolute());
        assert!(p.ends_with("videofetch"));
    }

    #[test]
    fn default_settings_roundtrip() {
        let s = Settings::default();
        let json = serde_json::to_string(&s).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.default_quality, "720");
        assert_eq!(back.last_category, "未分类");
        assert!(!back.bilibili_use_proxy);
    }
}
