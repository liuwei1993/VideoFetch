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
    let cancelled = false;
    const unsubs: Array<() => void> = [];

    (async () => {
      const u1 = await listen<DownloadProgress>("download-progress", (e) => {
        if (cancelled) return;
        if (e.payload.percent != null) setPercent(e.payload.percent);
        setLogs((prev) => {
          const next = [...prev, e.payload.line];
          return next.length > 200 ? next.slice(-200) : next;
        });
      });
      const u2 = await listen<DownloadFinished>("download-finished", (e) => {
        if (cancelled) return;
        setBusy(false);
        setDonePath(e.payload.path);
        setPercent(100);
        onCategoriesChanged();
      });
      const u3 = await listen<DownloadError>("download-error", (e) => {
        if (cancelled) return;
        setBusy(false);
        if (e.payload.message.includes("已停止")) {
          setLogs((prev) => [...prev, e.payload.message]);
          setError(null);
        } else {
          setError(e.payload.message);
        }
      });
      if (cancelled) {
        u1();
        u2();
        u3();
      } else {
        unsubs.push(u1, u2, u3);
      }
    })();

    return () => {
      cancelled = true;
      unsubs.forEach((u) => u());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- subscribe once; avoid dropping events on parent refresh
  }, []);

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

  async function onStop() {
    try {
      await api.stopDownload();
      setLogs((prev) => [...prev, "正在停止…"]);
    } catch (e) {
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
          disabled={busy}
        />
      </label>

      <div className="row-fields">
        <label className="field">
          <span>分类</span>
          <select
            value={category}
            onChange={(e) => setCategory(e.target.value)}
            disabled={busy}
          >
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
            disabled={busy}
          />
        </label>
        <label className="field">
          <span>清晰度</span>
          <select
            value={quality}
            onChange={(e) => setQuality(e.target.value)}
            disabled={busy}
          >
            <option value="720">720p</option>
            <option value="1080">1080p</option>
            <option value="best">最高</option>
          </select>
        </label>
      </div>

      <div className="actions-row">
        <button disabled={busy || !url.trim()} onClick={onStart}>
          {busy ? "下载中…" : "开始下载"}
        </button>
        <button className="danger" disabled={!busy} onClick={onStop}>
          停止
        </button>
      </div>

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
