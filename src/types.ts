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
