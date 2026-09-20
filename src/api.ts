import { invoke } from "@tauri-apps/api/core";
import type { DownloadQueue, Settings, VideoItem } from "./types";

export const api = {
  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) => invoke<void>("save_settings", { settings }),
  ensureLibrary: () => invoke<void>("ensure_library"),
  listCategories: () => invoke<string[]>("list_categories"),
  createCategory: (name: string) => invoke<void>("create_category", { name }),
  renameCategory: (from: string, to: string) =>
    invoke<void>("rename_category", { from, to }),
  deleteCategory: (name: string, force: boolean) =>
    invoke<void>("delete_category", { name, force }),
  listVideos: (category: string) =>
    invoke<VideoItem[]>("list_videos", { category }),
  moveVideo: (fromCategory: string, toCategory: string, filename: string) =>
    invoke<void>("move_video", {
      fromCategory,
      toCategory,
      filename,
    }),
  deleteVideo: (category: string, filename: string) =>
    invoke<void>("delete_video", { category, filename }),
  openVideo: (path: string) => invoke<void>("open_video", { path }),
  startDownload: (
    url: string,
    category: string,
    quality: string,
    audioOnly = false,
  ) =>
    invoke<void>("start_download", {
      args: { url, category, quality, audioOnly },
    }),
  stopDownload: () => invoke<void>("stop_download"),
  downloadRunning: () => invoke<boolean>("download_running"),
  getDownloadQueue: () => invoke<DownloadQueue | null>("get_download_queue"),
  resumeDownloadQueue: () => invoke<void>("resume_download_queue"),
  discardDownloadQueue: () => invoke<void>("discard_download_queue"),
};
