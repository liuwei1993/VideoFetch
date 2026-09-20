use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueKind {
    Batch,
    Single,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemStatus {
    Pending,
    Downloading,
    Done,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueItem {
    pub index: usize,
    pub id: String,
    pub title: String,
    pub url: String,
    pub status: ItemStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadQueue {
    pub version: u32,
    pub kind: QueueKind,
    pub page_url: String,
    pub category: String,
    pub quality: String,
    pub audio_only: bool,
    pub updated_at: String,
    pub items: Vec<QueueItem>,
}

impl DownloadQueue {
    pub fn is_resumable(&self) -> bool {
        self.items.iter().any(|i| {
            matches!(
                i.status,
                ItemStatus::Pending | ItemStatus::Downloading | ItemStatus::Failed
            )
        })
    }

    pub fn set_item_status(&mut self, index: usize, status: ItemStatus) {
        if let Some(item) = self.items.iter_mut().find(|i| i.index == index) {
            item.status = status;
        }
    }

    pub fn touch(&mut self) {
        self.updated_at = unix_now();
    }
}

fn unix_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}

pub fn queue_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("config dir: {e}"))?;
    Ok(dir.join("download_queue.json"))
}

pub fn load_queue_from(path: &Path) -> Result<Option<DownloadQueue>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let raw = match fs::read_to_string(path) {
        Ok(r) => r,
        Err(e) => return Err(format!("read queue: {e}")),
    };
    match serde_json::from_str::<DownloadQueue>(&raw) {
        Ok(q) => Ok(Some(q)),
        Err(e) => {
            eprintln!("download_queue.json corrupt, ignoring: {e}");
            Ok(None)
        }
    }
}

pub fn save_queue_to(path: &Path, queue: &DownloadQueue) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create config dir: {e}"))?;
    }
    let raw = serde_json::to_string_pretty(queue).map_err(|e| e.to_string())?;
    fs::write(path, raw).map_err(|e| format!("write queue: {e}"))
}

pub fn clear_queue_at(path: &Path) -> Result<(), String> {
    if path.exists() {
        fs::remove_file(path).map_err(|e| format!("remove queue: {e}"))?;
    }
    Ok(())
}

pub fn load_queue(app: &AppHandle) -> Result<Option<DownloadQueue>, String> {
    load_queue_from(&queue_path(app)?)
}

pub fn save_queue(app: &AppHandle, queue: &DownloadQueue) -> Result<(), String> {
    save_queue_to(&queue_path(app)?, queue)
}

pub fn clear_queue(app: &AppHandle) -> Result<(), String> {
    clear_queue_at(&queue_path(app)?)
}

/// Returns queue only if resumable; otherwise None (and optionally clear all-done file).
pub fn load_resumable_queue(app: &AppHandle) -> Result<Option<DownloadQueue>, String> {
    match load_queue(app)? {
        Some(q) if q.is_resumable() => Ok(Some(q)),
        Some(_) => {
            let _ = clear_queue(app);
            Ok(None)
        }
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_queue_path() -> PathBuf {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("videofetch_queue_test_{n}.json"))
    }

    fn sample_batch() -> DownloadQueue {
        DownloadQueue {
            version: 1,
            kind: QueueKind::Batch,
            page_url: "https://space.bilibili.com/1/lists/2?type=season".into(),
            category: "测试".into(),
            quality: "720".into(),
            audio_only: false,
            updated_at: "2026-09-20T00:00:00Z".into(),
            items: vec![
                QueueItem {
                    index: 0,
                    id: "BV1".into(),
                    title: "一".into(),
                    url: "https://www.bilibili.com/video/BV1".into(),
                    status: ItemStatus::Done,
                },
                QueueItem {
                    index: 1,
                    id: "BV2".into(),
                    title: "二".into(),
                    url: "https://www.bilibili.com/video/BV2".into(),
                    status: ItemStatus::Pending,
                },
            ],
        }
    }

    #[test]
    fn roundtrip_save_load() {
        let path = temp_queue_path();
        let q = sample_batch();
        save_queue_to(&path, &q).unwrap();
        let back = load_queue_from(&path).unwrap().unwrap();
        assert_eq!(back.kind, QueueKind::Batch);
        assert_eq!(back.items.len(), 2);
        assert_eq!(back.items[1].status, ItemStatus::Pending);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn is_resumable_requires_incomplete() {
        let mut q = sample_batch();
        assert!(q.is_resumable());
        q.items[1].status = ItemStatus::Done;
        assert!(!q.is_resumable());
        q.items[1].status = ItemStatus::Failed;
        assert!(q.is_resumable());
    }

    #[test]
    fn corrupt_or_missing_returns_none() {
        let path = temp_queue_path();
        assert!(load_queue_from(&path).unwrap().is_none());
        fs::write(&path, "{not json").unwrap();
        assert!(load_queue_from(&path).unwrap().is_none());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn clear_removes_file() {
        let path = temp_queue_path();
        save_queue_to(&path, &sample_batch()).unwrap();
        clear_queue_at(&path).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn set_item_status_updates() {
        let mut q = sample_batch();
        q.set_item_status(1, ItemStatus::Downloading);
        assert_eq!(q.items[1].status, ItemStatus::Downloading);
    }
}
