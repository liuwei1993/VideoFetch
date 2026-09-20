import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api } from "../api";
import type { DownloadError, DownloadFinished, DownloadProgress } from "../types";

type Props = {
  categories: string[];
  defaultCategory: string;
  defaultQuality: string;
  onCategoriesChanged: () => void;
};

export function DownloadView({
  categories,
  defaultCategory,
  defaultQuality,
  onCategoriesChanged,
}: Props) {
  const [url, setUrl] = useState("");
  const [category, setCategory] = useState(defaultCategory);
  const [newCategory, setNewCategory] = useState("");
  const [quality, setQuality] = useState(defaultQuality);
  const [percent, setPercent] = useState<number | null>(null);
  const [logs, setLogs] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [donePath, setDonePath] = useState<string | null>(null);

  useEffect(() => {
    setCategory(defaultCategory);
  }, [defaultCategory]);

  useEffect(() => {
    setQuality(defaultQuality);
  }, [defaultQuality]);

  useEffect(() => {
    const unsubs: Array<() => void> = [];
    listen<DownloadProgress>("download-progress", (e) => {
      if (e.payload.percent != null) setPercent(e.payload.percent);
      setLogs((prev) => [...prev.slice(-200), e.payload.line]);
    }).then((u) => unsubs.push(u));
    listen<DownloadFinished>("download-finished", (e) => {
      setBusy(false);
      setDonePath(e.payload.path);
      setPercent(100);
      onCategoriesChanged();
    }).then((u) => unsubs.push(u));
    listen<DownloadError>("download-error", (e) => {
      setBusy(false);
      setError(e.payload.message);
    }).then((u) => unsubs.push(u));
    return () => unsubs.forEach((u) => u());
  }, [onCategoriesChanged]);

  async function ensureCategorySelected(): Promise<string> {
    const name = newCategory.trim();
    if (name) {
      await api.createCategory(name);
      setNewCategory("");
      onCategoriesChanged();
      setCategory(name);
      return name;
    }
    return category || "未分类";
  }

  async function onStart() {
    setError(null);
    setDonePath(null);
    setLogs([]);
    setPercent(0);
    try {
      const cat = await ensureCategorySelected();
      setBusy(true);
      await api.startDownload(url.trim(), cat, quality);
    } catch (e) {
      setBusy(false);
      setError(String(e));
    }
  }

  return (
    <section className="panel">
      <h2>下载</h2>
      <label className="field">
        <span>视频链接</span>
        <input
          value={url}
          onChange={(e) => setUrl(e.target.value)}
          placeholder="YouTube / Bilibili URL"
        />
      </label>

      <div className="row-fields">
        <label className="field">
          <span>分类</span>
          <select value={category} onChange={(e) => setCategory(e.target.value)}>
            {categories.map((c) => (
              <option key={c} value={c}>
                {c}
              </option>
            ))}
          </select>
        </label>
        <label className="field">
          <span>或新建分类</span>
          <input
            value={newCategory}
            onChange={(e) => setNewCategory(e.target.value)}
            placeholder="新分类名"
          />
        </label>
        <label className="field">
          <span>清晰度</span>
          <select value={quality} onChange={(e) => setQuality(e.target.value)}>
            <option value="720">720p</option>
            <option value="1080">1080p</option>
            <option value="best">最高</option>
          </select>
        </label>
      </div>

      <button disabled={busy || !url.trim()} onClick={onStart}>
        {busy ? "下载中…" : "开始下载"}
      </button>

      {percent != null && (
        <div className="progress">
          <div className="progress-bar" style={{ width: `${Math.min(percent, 100)}%` }} />
          <span>{percent.toFixed(1)}%</span>
        </div>
      )}

      {error && <p className="error">{error}</p>}
      {donePath && <p className="ok">完成：{donePath}</p>}

      <pre className="log">{logs.join("\n")}</pre>
    </section>
  );
}
