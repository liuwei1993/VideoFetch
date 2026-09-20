import { useCallback, useEffect, useState } from "react";
import { Alert, Layout, Menu, Typography, theme } from "antd";
import {
  CloudDownloadOutlined,
  FolderOpenOutlined,
  SettingOutlined,
  VideoCameraOutlined,
} from "@ant-design/icons";
import { api } from "./api";
import type { Settings } from "./types";
import { DownloadView } from "./views/DownloadView";
import { LibraryView } from "./views/LibraryView";
import { SettingsView } from "./views/SettingsView";

const { Header, Content } = Layout;

type Tab = "download" | "library" | "settings";

function App() {
  const [tab, setTab] = useState<Tab>("download");
  const [settings, setSettings] = useState<Settings | null>(null);
  const [categories, setCategories] = useState<string[]>([]);
  const [bootError, setBootError] = useState<string | null>(null);
  const { token } = theme.useToken();

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
    <Layout className="app-shell">
      <Header className="app-header" style={{ background: token.colorBgContainer }}>
        <div className="brand">
          <VideoCameraOutlined className="brand-icon" />
          <Typography.Title level={4} style={{ margin: 0 }}>
            VideoFetch
          </Typography.Title>
        </div>
        <Menu
          mode="horizontal"
          selectedKeys={[tab]}
          onClick={({ key }) => setTab(key as Tab)}
          items={[
            {
              key: "download",
              icon: <CloudDownloadOutlined />,
              label: "下载",
            },
            {
              key: "library",
              icon: <FolderOpenOutlined />,
              label: "库",
            },
            {
              key: "settings",
              icon: <SettingOutlined />,
              label: "设置",
            },
          ]}
          style={{ flex: 1, minWidth: 0, justifyContent: "flex-end", border: 0 }}
        />
      </Header>

      <Content className="app-content">
        {bootError && (
          <Alert
            type="error"
            showIcon
            message="启动失败"
            description={bootError}
            style={{ marginBottom: 16 }}
          />
        )}

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
      </Content>
    </Layout>
  );
}

export default App;
