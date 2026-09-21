use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

pub const JOB_STORE_VERSION: u32 = 2;

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

    #[allow(dead_code)]
    pub fn touch(&mut self) {
        self.updated_at = unix_now();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Pending,
    Downloading,
    Done,
    Failed,
    Cancelled,
}

impl JobStatus {
    pub fn is_resumable(self) -> bool {
        matches!(self, JobStatus::Pending | JobStatus::Downloading | JobStatus::Failed)
    }

    pub fn is_active(self) -> bool {
        matches!(self, JobStatus::Pending | JobStatus::Downloading)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadJob {
    pub id: String,
    pub url: String,
    pub category: String,
    pub quality: String,
    pub audio_only: bool,
    pub kind: QueueKind,
    pub status: JobStatus,
    pub title: String,
    #[serde(default)]
    pub percent: Option<f64>,
    #[serde(default)]
    pub speed: Option<String>,
    #[serde(default)]
    pub eta: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub detail: String,
    #[serde(default)]
    pub completed: usize,
    #[serde(default)]
    pub total: usize,
    #[serde(default)]
    pub items: Vec<QueueItem>,
    #[serde(default)]
    pub updated_at: String,
}

impl DownloadJob {
    pub fn new(
        id: String,
        url: String,
        category: String,
        quality: String,
        audio_only: bool,
        kind: QueueKind,
    ) -> Self {
        let title = queue_item_id_from_url(&url);
        Self {
            id,
            url: url.clone(),
            category,
            quality,
            audio_only,
            kind,
            status: JobStatus::Pending,
            title,
            percent: Some(0.0),
            speed: None,
            eta: None,
            error: None,
            path: None,
            detail: String::new(),
            completed: 0,
            total: 1,
            items: Vec::new(),
            updated_at: unix_now(),
        }
    }

    pub fn is_resumable(&self) -> bool {
        self.status.is_resumable()
    }

    pub fn touch(&mut self) {
        self.updated_at = unix_now();
    }

    pub fn set_item_status(&mut self, index: usize, status: ItemStatus) {
        if let Some(item) = self.items.iter_mut().find(|i| i.index == index) {
            item.status = status;
        }
        self.completed = self
            .items
            .iter()
            .filter(|i| i.status == ItemStatus::Done)
            .count();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobStore {
    pub version: u32,
    pub jobs: Vec<DownloadJob>,
}

impl Default for JobStore {
    fn default() -> Self {
        Self {
            version: JOB_STORE_VERSION,
            jobs: Vec::new(),
        }
    }
}

pub fn job_from_legacy_queue(queue: DownloadQueue) -> DownloadJob {
    let done = queue
        .items
        .iter()
        .filter(|i| i.status == ItemStatus::Done)
        .count();
    let title = queue
        .items
        .first()
        .map(|i| i.title.clone())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| queue.page_url.clone());
    let status = if queue.is_resumable() {
        JobStatus::Pending
    } else {
        JobStatus::Done
    };
    DownloadJob {
        id: "job-legacy".into(),
        url: queue.page_url,
        category: queue.category,
        quality: queue.quality,
        audio_only: queue.audio_only,
        kind: queue.kind,
        status,
        title,
        percent: None,
        speed: None,
        eta: None,
        error: None,
        path: None,
        detail: String::new(),
        completed: done,
        total: queue.items.len().max(1),
        items: queue.items,
        updated_at: queue.updated_at,
    }
}

pub fn parse_jobs_raw(raw: &str) -> Option<JobStore> {
    if let Ok(store) = serde_json::from_str::<JobStore>(raw) {
        if store.version >= JOB_STORE_VERSION {
            return Some(store);
        }
    }
    match serde_json::from_str::<DownloadQueue>(raw) {
        Ok(q) => Some(JobStore {
            version: JOB_STORE_VERSION,
            jobs: vec![job_from_legacy_queue(q)],
        }),
        Err(_) => None,
    }
}

fn queue_item_id_from_url(url: &str) -> String {
    let base = url
        .split('?')
        .next()
        .unwrap_or(url)
        .split('#')
        .next()
        .unwrap_or(url);
    base.rsplit('/')
        .find(|s| !s.is_empty())
        .unwrap_or(url)
        .to_string()
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

pub fn jobs_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("config dir: {e}"))?;
    Ok(dir.join("download_jobs.json"))
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

#[allow(dead_code)]
pub fn load_queue(app: &AppHandle) -> Result<Option<DownloadQueue>, String> {
    load_queue_from(&queue_path(app)?)
}

#[allow(dead_code)]
pub fn save_queue(app: &AppHandle, queue: &DownloadQueue) -> Result<(), String> {
    save_queue_to(&queue_path(app)?, queue)
}

pub fn clear_queue(app: &AppHandle) -> Result<(), String> {
    clear_queue_at(&queue_path(app)?)
}

/// Returns queue only if resumable; otherwise None (and optionally clear all-done file).
#[allow(dead_code)]
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

pub fn load_job_store_from(jobs_file: &Path, legacy_file: &Path) -> Result<JobStore, String> {
    if jobs_file.exists() {
        let raw = fs::read_to_string(jobs_file).map_err(|e| format!("read jobs: {e}"))?;
        return Ok(parse_jobs_raw(&raw).unwrap_or_default());
    }
    if legacy_file.exists() {
        let raw = fs::read_to_string(legacy_file).map_err(|e| format!("read queue: {e}"))?;
        if let Some(store) = parse_jobs_raw(&raw) {
            let _ = save_job_store_to(jobs_file, &store);
            let _ = clear_queue_at(legacy_file);
            return Ok(store);
        }
    }
    Ok(JobStore::default())
}

pub fn save_job_store_to(path: &Path, store: &JobStore) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create config dir: {e}"))?;
    }
    let raw = serde_json::to_string_pretty(store).map_err(|e| e.to_string())?;
    fs::write(path, raw).map_err(|e| format!("write jobs: {e}"))
}

pub fn clear_job_store_at(path: &Path) -> Result<(), String> {
    if path.exists() {
        fs::remove_file(path).map_err(|e| format!("remove jobs: {e}"))?;
    }
    Ok(())
}

pub fn load_job_store(app: &AppHandle) -> Result<JobStore, String> {
    load_job_store_from(&jobs_path(app)?, &queue_path(app)?)
}

pub fn save_incomplete_jobs(app: &AppHandle, jobs: &[DownloadJob]) -> Result<(), String> {
    let incomplete: Vec<DownloadJob> = jobs
        .iter()
        .filter(|j| j.is_resumable())
        .cloned()
        .collect();
    let path = jobs_path(app)?;
    if incomplete.is_empty() {
        clear_job_store_at(&path)?;
        let _ = clear_queue(app);
        return Ok(());
    }
    save_job_store_to(
        &path,
        &JobStore {
            version: JOB_STORE_VERSION,
            jobs: incomplete,
        },
    )
}

#[allow(dead_code)]
pub fn load_resumable_jobs(app: &AppHandle) -> Result<Vec<DownloadJob>, String> {
    let store = load_job_store(app)?;
    let jobs: Vec<DownloadJob> = store
        .jobs
        .into_iter()
        .filter(|j| j.is_resumable())
        .collect();
    Ok(jobs)
}

pub fn discard_job_temps(library_root: &Path, job: &DownloadJob) -> Result<(), String> {
    let cat = library_root.join(&job.category);
    let ids: Vec<String> = if job.items.is_empty() {
        vec![job.title.clone()]
    } else {
        job.items
            .iter()
            .filter(|i| i.status != ItemStatus::Done)
            .map(|i| i.id.clone())
            .collect()
    };
    cleanup_temp_files_in_category(&cat, &ids)
}

/// Remove `.part` / `.ytdl` (and similar) whose filename contains `[id]` for given ids.
pub fn cleanup_temp_files_in_category(category_dir: &Path, ids: &[String]) -> Result<(), String> {
    if !category_dir.is_dir() {
        return Ok(());
    }
    let entries = fs::read_dir(category_dir).map_err(|e| e.to_string())?;
    for ent in entries.flatten() {
        let name = ent.file_name().to_string_lossy().into_owned();
        let lower = name.to_lowercase();
        let is_temp = lower.ends_with(".part")
            || lower.ends_with(".ytdl")
            || lower.contains(".part.")
            || looks_like_incomplete(&name);
        if !is_temp {
            continue;
        }
        let matched = ids.iter().any(|id| name.contains(&format!("[{id}]")));
        if matched {
            let _ = fs::remove_file(ent.path());
        }
    }
    Ok(())
}

fn looks_like_incomplete(name: &str) -> bool {
    crate::library::looks_like_ytdlp_fragment(name)
}

#[allow(dead_code)]
pub fn discard_queue_temps(library_root: &Path, queue: &DownloadQueue) -> Result<(), String> {
    let cat = library_root.join(&queue.category);
    let ids: Vec<String> = queue
        .items
        .iter()
        .filter(|i| i.status != ItemStatus::Done)
        .map(|i| i.id.clone())
        .collect();
    cleanup_temp_files_in_category(&cat, &ids)
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

    #[test]
    fn cleanup_temp_files_removes_part_and_ytdl_matching_ids() {
        let root = std::env::temp_dir().join(format!(
            "vf_cleanup_{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let cat = root.join("分类");
        fs::create_dir_all(&cat).unwrap();
        let part = cat.join("foo [BV2].mp4.part");
        let ytdl = cat.join("foo [BV2].mp4.ytdl");
        let keep = cat.join("foo [BV1].mp4");
        let other = cat.join("bar [BV9].mp4.part");
        fs::write(&part, b"x").unwrap();
        fs::write(&ytdl, b"x").unwrap();
        fs::write(&keep, b"x").unwrap();
        fs::write(&other, b"x").unwrap();

        cleanup_temp_files_in_category(&cat, &["BV2".into()]).unwrap();

        assert!(!part.exists());
        assert!(!ytdl.exists());
        assert!(keep.exists());
        assert!(other.exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn parse_jobs_raw_migrates_legacy_queue() {
        let q = sample_batch();
        let raw = serde_json::to_string(&q).unwrap();
        let store = parse_jobs_raw(&raw).unwrap();
        assert_eq!(store.version, JOB_STORE_VERSION);
        assert_eq!(store.jobs.len(), 1);
        let job = &store.jobs[0];
        assert_eq!(job.id, "job-legacy");
        assert_eq!(job.kind, QueueKind::Batch);
        assert_eq!(job.status, JobStatus::Pending);
        assert_eq!(job.completed, 1);
        assert_eq!(job.total, 2);
        assert_eq!(job.items.len(), 2);
    }

    #[test]
    fn parse_jobs_raw_reads_version_two_store() {
        let job = job_from_legacy_queue(sample_batch());
        let store = JobStore {
            version: JOB_STORE_VERSION,
            jobs: vec![job],
        };
        let raw = serde_json::to_string(&store).unwrap();
        let back = parse_jobs_raw(&raw).unwrap();
        assert_eq!(back.jobs.len(), 1);
        assert_eq!(back.jobs[0].id, "job-legacy");
    }

    #[test]
    fn load_job_store_migrates_legacy_file() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("vf_jobs_mig_{n}"));
        fs::create_dir_all(&dir).unwrap();
        let jobs_file = dir.join("download_jobs.json");
        let legacy = dir.join("download_queue.json");
        save_queue_to(&legacy, &sample_batch()).unwrap();

        let store = load_job_store_from(&jobs_file, &legacy).unwrap();
        assert_eq!(store.jobs.len(), 1);
        assert!(jobs_file.exists());
        assert!(!legacy.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn job_status_resumable() {
        assert!(JobStatus::Pending.is_resumable());
        assert!(JobStatus::Downloading.is_resumable());
        assert!(JobStatus::Failed.is_resumable());
        assert!(!JobStatus::Done.is_resumable());
        assert!(!JobStatus::Cancelled.is_resumable());
    }
}
