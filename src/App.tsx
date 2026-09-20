import { useCallback, useEffect, useState } from "react";
import { Alert, Layout, Menu, Modal, Typography, message, theme } from "antd";
import {
  CloudDownloadOutlined,
  FolderOpenOutlined,
  SettingOutlined,
  VideoCameraOutlined,
} from "@ant-design/icons";
import { api } from "./api";
import type { DownloadQueue, Settings } from "./types";
import { DownloadView } from "./views/DownloadView";
import { LibraryView } from "./views/LibraryView";
import { SettingsView } from "./views/SettingsView";

const { Header, Content } = Layout;

type Tab = "download" | "library" | "settings";

function queueSummary(q: DownloadQueue): string {
  const done = q.items.filter((i) => i.status === "done").length;
  const todo = q.items.length - done;
  if (q.kind === "batch") {
    return `合集 · 共 ${q.items.length} · 已完成 ${done} · 待处理 ${todo} · 分类「${q.category}」`;
  }
  const title = q.items[0]?.title || q.page_url;
  return `单视频 · ${title} · 分类「${q.category}」`;
}

function App() {
  const [tab, setTab] = useState<Tab>("download");
  const [settings, setSettings] = useState<Settings | null>(null);
  const [categories, setCategories] = useState<string[]>([]);
  const [bootError, setBootError] = useState<string | null>(null);
  const [pendingQueue, setPendingQueue] = useState<DownloadQueue | null>(null);
  const [resumeSeed, setResumeSeed] = useState<DownloadQueue | null>(null);
  const [resuming, setResuming] = useState(false);
  const [resumeResetKey, setResumeResetKey] = useState(0);
  const { token } = theme.useToken();

  const refresh = useCallback(async () => {
    const [s, cats] = await Promise.all([
      api.getSettings(),
      api.listCategories(),
    ]);
    setSettings(s);
    setCategories(cats);
  }, []);

  const onResumeSeedConsumed = useCallback(() => setResumeSeed(null), []);

  useEffect(() => {
    (async () => {
      try {
        await api.ensureLibrary();
        await refresh();
        const q = await api.getDownloadQueue();
        if (q) setPendingQueue(q);
      } catch (e) {
        setBootError(String(e));
      }
    })();
  }, [refresh]);

  async function onResumeQueue() {
    if (!pendingQueue) return;
    setResuming(true);
    try {
      setTab("download");
      setResumeSeed(pendingQueue);
      await new Promise((r) => setTimeout(r, 50));
      await api.resumeDownloadQueue();
      setPendingQueue(null);
    } catch (e) {
      setResumeSeed(null);
      setResumeResetKey((k) => k + 1);
      message.error(String(e));
    } finally {
      setResuming(false);
    }
  }

  async function onDiscardQueue() {
    if (resuming) return;
    try {
      await api.discardDownloadQueue();
      setPendingQueue(null);
    } catch (e) {
      setBootError(String(e));
    }
  }

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
            resumeSeed={resumeSeed}
            onResumeSeedConsumed={onResumeSeedConsumed}
            resumeResetKey={resumeResetKey}
          />
        )}
        {tab === "library" && (
          <LibraryView categories={categories} onCategoriesChanged={refresh} />
        )}
        {tab === "settings" && <SettingsView onSaved={refresh} />}
      </Content>

      <Modal
        title="未完成的下载"
        open={!!pendingQueue}
        okText="继续"
        cancelText="丢弃"
        onOk={onResumeQueue}
        onCancel={onDiscardQueue}
        confirmLoading={resuming}
        cancelButtonProps={{ disabled: resuming }}
        closable={false}
        maskClosable={false}
      >
        {pendingQueue && <p>{queueSummary(pendingQueue)}</p>}
      </Modal>
    </Layout>
  );
}

export default App;
