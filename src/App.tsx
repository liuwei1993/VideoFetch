import { useCallback, useEffect, useState } from "react";
import { api } from "./api";
import type { Settings } from "./types";
import { DownloadView } from "./views/DownloadView";
import { LibraryView } from "./views/LibraryView";
import { SettingsView } from "./views/SettingsView";
import "./App.css";

type Tab = "download" | "library" | "settings";

function App() {
  const [tab, setTab] = useState<Tab>("download");
  const [settings, setSettings] = useState<Settings | null>(null);
  const [categories, setCategories] = useState<string[]>([]);
  const [bootError, setBootError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    const [s, cats] = await Promise.all([
      api.getSettings(),
      api.listCategories(),
    ]);
    setSettings(s);
    setCategories(cats);
  }, []);

  useEffect(() => {
    (async () => {
      try {
        await api.ensureLibrary();
        await refresh();
      } catch (e) {
        setBootError(String(e));
      }
    })();
  }, [refresh]);

  return (
    <div className="app">
      <header className="topbar">
        <div className="brand">VideoFetch</div>
        <nav>
          <button
            className={tab === "download" ? "active" : ""}
            onClick={() => setTab("download")}
          >
            下载
          </button>
          <button
            className={tab === "library" ? "active" : ""}
            onClick={() => setTab("library")}
          >
            库
          </button>
          <button
            className={tab === "settings" ? "active" : ""}
            onClick={() => setTab("settings")}
          >
            设置
          </button>
        </nav>
      </header>

      {bootError && <p className="error pad">{bootError}</p>}

      {tab === "download" && settings && (
        <DownloadView
          categories={categories}
          defaultCategory={settings.last_category || "未分类"}
          defaultQuality={settings.default_quality || "720"}
          onCategoriesChanged={refresh}
        />
      )}
      {tab === "library" && (
        <LibraryView categories={categories} onCategoriesChanged={refresh} />
      )}
      {tab === "settings" && <SettingsView onSaved={refresh} />}
    </div>
  );
}

export default App;
