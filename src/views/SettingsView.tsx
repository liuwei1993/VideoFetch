import { useEffect, useState } from "react";
import { api } from "../api";
import type { Settings } from "../types";

type Props = {
  onSaved: () => void;
};

export function SettingsView({ onSaved }: Props) {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.getSettings().then(setSettings).catch((e) => setError(String(e)));
  }, []);

  if (!settings) {
    return (
      <section className="panel">
        <h2>设置</h2>
        {error ? <p className="error">{error}</p> : <p>加载中…</p>}
      </section>
    );
  }

  async function onSave() {
    if (!settings) return;
    setError(null);
    setMessage(null);
    try {
      await api.saveSettings(settings);
      setMessage("已保存");
      onSaved();
    } catch (e) {
      setError(String(e));
    }
  }

  return (
    <section className="panel">
      <h2>设置</h2>
      <label className="field">
        <span>库根目录</span>
        <input
          value={settings.library_root}
          onChange={(e) => setSettings({ ...settings, library_root: e.target.value })}
        />
      </label>
      <label className="field">
        <span>默认清晰度</span>
        <select
          value={settings.default_quality}
          onChange={(e) =>
            setSettings({ ...settings, default_quality: e.target.value })
          }
        >
          <option value="720">720p</option>
          <option value="1080">1080p</option>
          <option value="best">最高</option>
        </select>
      </label>
      <label className="field">
        <span>YouTube 代理</span>
        <input
          value={settings.youtube_proxy}
          onChange={(e) =>
            setSettings({ ...settings, youtube_proxy: e.target.value })
          }
          placeholder="http://127.0.0.1:57890"
        />
      </label>
      <label className="field checkbox">
        <input
          type="checkbox"
          checked={settings.bilibili_use_proxy}
          onChange={(e) =>
            setSettings({ ...settings, bilibili_use_proxy: e.target.checked })
          }
        />
        <span>Bilibili 也走代理</span>
      </label>
      <label className="field">
        <span>Cookie 文件（预留）</span>
        <input
          value={settings.cookie_file ?? ""}
          onChange={(e) =>
            setSettings({
              ...settings,
              cookie_file: e.target.value.trim() ? e.target.value : null,
            })
          }
          placeholder="可选，第一版下载暂不接入"
        />
      </label>
      <p className="muted">上次分类：{settings.last_category}</p>
      <button onClick={onSave}>保存</button>
      {message && <p className="ok">{message}</p>}
      {error && <p className="error">{error}</p>}
    </section>
  );
}
