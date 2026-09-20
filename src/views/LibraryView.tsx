import { useEffect, useState } from "react";
import { api } from "../api";
import type { VideoItem } from "../types";

type Props = {
  categories: string[];
  onCategoriesChanged: () => void;
};

function formatSize(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`;
  return `${(n / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

export function LibraryView({ categories, onCategoriesChanged }: Props) {
  const [active, setActive] = useState(categories[0] || "未分类");
  const [videos, setVideos] = useState<VideoItem[]>([]);
  const [newName, setNewName] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [moveTarget, setMoveTarget] = useState<Record<string, string>>({});

  useEffect(() => {
    if (!categories.includes(active) && categories.length) {
      setActive(categories[0]);
    }
  }, [categories, active]);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const list = await api.listVideos(active);
        if (!cancelled) setVideos(list);
      } catch (e) {
        if (!cancelled) setError(String(e));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [active]);

  async function refreshVideos() {
    setVideos(await api.listVideos(active));
  }

  async function onCreate() {
    setError(null);
    try {
      await api.createCategory(newName.trim());
      setNewName("");
      onCategoriesChanged();
    } catch (e) {
      setError(String(e));
    }
  }

  async function onDeleteCategory() {
    if (active === "未分类") return;
    if (!confirm(`删除分类「${active}」？非空将强制删除。`)) return;
    setError(null);
    try {
      await api.deleteCategory(active, true);
      onCategoriesChanged();
      setActive("未分类");
    } catch (e) {
      setError(String(e));
    }
  }

  async function onRenameCategory() {
    if (active === "未分类") return;
    const to = prompt("新分类名", active);
    if (!to || to === active) return;
    setError(null);
    try {
      await api.renameCategory(active, to);
      onCategoriesChanged();
      setActive(to);
    } catch (e) {
      setError(String(e));
    }
  }

  return (
    <section className="panel library">
      <aside className="sidebar">
        <h2>分类</h2>
        <ul>
          {categories.map((c) => (
            <li key={c}>
              <button
                className={c === active ? "active" : ""}
                onClick={() => setActive(c)}
              >
                {c}
              </button>
            </li>
          ))}
        </ul>
        <div className="sidebar-actions">
          <input
            value={newName}
            onChange={(e) => setNewName(e.target.value)}
            placeholder="新分类"
          />
          <button onClick={onCreate} disabled={!newName.trim()}>
            新建
          </button>
          <button onClick={onRenameCategory} disabled={active === "未分类"}>
            重命名
          </button>
          <button onClick={onDeleteCategory} disabled={active === "未分类"}>
            删除分类
          </button>
        </div>
      </aside>

      <div className="video-list">
        <h2>{active}</h2>
        {error && <p className="error">{error}</p>}
        {videos.length === 0 && <p className="muted">暂无视频</p>}
        <ul>
          {videos.map((v) => (
            <li key={v.path} className="video-row">
              <div>
                <strong>{v.name}</strong>
                <div className="muted">{formatSize(v.size)}</div>
              </div>
              <div className="video-actions">
                <button onClick={() => api.openVideo(v.path)}>打开</button>
                <select
                  value={moveTarget[v.name] || ""}
                  onChange={(e) =>
                    setMoveTarget((m) => ({ ...m, [v.name]: e.target.value }))
                  }
                >
                  <option value="">移动到…</option>
                  {categories
                    .filter((c) => c !== active)
                    .map((c) => (
                      <option key={c} value={c}>
                        {c}
                      </option>
                    ))}
                </select>
                <button
                  disabled={!moveTarget[v.name]}
                  onClick={async () => {
                    try {
                      await api.moveVideo(active, moveTarget[v.name], v.name);
                      await refreshVideos();
                      onCategoriesChanged();
                    } catch (e) {
                      setError(String(e));
                    }
                  }}
                >
                  移动
                </button>
                <button
                  className="danger"
                  onClick={async () => {
                    if (!confirm(`删除 ${v.name}？`)) return;
                    try {
                      await api.deleteVideo(active, v.name);
                      await refreshVideos();
                    } catch (e) {
                      setError(String(e));
                    }
                  }}
                >
                  删除
                </button>
              </div>
            </li>
          ))}
        </ul>
      </div>
    </section>
  );
}
