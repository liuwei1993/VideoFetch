export type Settings = {
  library_root: string;
  default_quality: string;
  last_category: string;
  youtube_proxy: string;
  bilibili_use_proxy: boolean;
  cookie_file: string | null;
  max_concurrent_downloads: number;
};

export type VideoItem = {
  name: string;
  path: string;
  size: number;
};

export type DownloadProgress = {
  percent: number | null;
  line: string;
  speed: string | null;
  eta: string | null;
};

export type DownloadFinished = {
  path: string;
};

export type DownloadError = {
  message: string;
};

export type QueueKind = "batch" | "single";
export type ItemStatus = "pending" | "downloading" | "done" | "failed";

export type QueueItem = {
  index: number;
  id: string;
  title: string;
  url: string;
  status: ItemStatus;
};

export type DownloadQueue = {
  version: number;
  kind: QueueKind;
  page_url: string;
  category: string;
  quality: string;
  audio_only: boolean;
  updated_at: string;
  items: QueueItem[];
};

export type BatchItemMeta = {
  id: string;
  title: string;
  status?: string | null;
};
export type DownloadBatchStarted = { total: number; items: BatchItemMeta[] };
export type DownloadItemStarted = { index: number; id: string };
export type DownloadItemFinished = { index: number; id: string; path: string };
export type DownloadItemError = { index: number; id: string; message: string };
export type DownloadBatchFinished = {
  succeeded: number;
  failed: number;
  cancelled: number;
};
