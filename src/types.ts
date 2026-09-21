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

export type JobStatus =
  | "pending"
  | "downloading"
  | "done"
  | "failed"
  | "cancelled";

export type DownloadJob = {
  id: string;
  url: string;
  category: string;
  quality: string;
  audioOnly: boolean;
  kind: QueueKind;
  status: JobStatus;
  title: string;
  percent: number | null;
  speed: string | null;
  eta: string | null;
  error: string | null;
  path: string | null;
  detail: string;
  completed: number;
  total: number;
  items: QueueItem[];
  updatedAt: string;
};

export type DownloadProgress = {
  jobId: string;
  percent: number | null;
  line: string;
  speed: string | null;
  eta: string | null;
};

export type DownloadFinished = {
  jobId: string;
  path: string;
};

export type DownloadError = {
  jobId: string;
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

export type BatchItemMeta = {
  id: string;
  title: string;
  status?: string | null;
};
export type DownloadBatchStarted = {
  jobId: string;
  total: number;
  items: BatchItemMeta[];
};
export type DownloadItemStarted = { jobId: string; index: number; id: string };
export type DownloadItemFinished = {
  jobId: string;
  index: number;
  id: string;
  path: string;
};
export type DownloadItemError = {
  jobId: string;
  index: number;
  id: string;
  message: string;
};
export type DownloadBatchFinished = {
  jobId: string;
  succeeded: number;
  failed: number;
  cancelled: number;
};
