import { useCallback, useEffect, useState } from "react";
import { Alert, Layout, Menu, Modal, Typography, message, theme } from "antd";
import {
  CloudDownloadOutlined,
  FolderOpenOutlined,
  SettingOutlined,
  VideoCameraOutlined,
} from "@ant-design/icons";
import { api } from "./api";
import type { DownloadJob, Settings } from "./types";
import { DownloadView } from "./views/DownloadView";
import { LibraryView } from "./views/LibraryView";
import { SettingsView } from "./views/SettingsView";

const { Header, Content } = Layout;

type Tab = "download" | "library" | "settings";

function jobsSummary(jobs: DownloadJob[]): string {
  if (jobs.length === 1) {
    const j = jobs[0];
    const kind = j.kind === "batch" ? "合集" : "单视频";
    return `${kind} · ${j.title || j.url} · 分类「${j.category}」`;
  }
  return `${jobs.length} 个未完成任务`;
}

function App() {
  const [tab, setTab] = useState<Tab>("download");
  const [settings, setSettings] = useState<Settings | null>(null);
  const [categories, setCategories] = useState<string[]>([]);
  const [bootError, setBootError] = useState<string | null>(null);
  const [pendingJobs, setPendingJobs] = useState<DownloadJob[]>([]);
  const [resuming, setResuming] = useState(false);
  const [resumeTick, setResumeTick] = useState(0);
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
        const jobs = await api.getDownloadQueue();
        if (jobs.length) setPendingJobs(jobs);
      } catch (e) {
        setBootError(String(e));
      }
    })();
  }, [refresh]);

  async function onResumeQueue() {
    if (!pendingJobs.length) return;
    setResuming(true);
    try {
      setTab("download");
      await api.resumeDownloadQueue();
      setPendingJobs([]);
      setResumeTick((n) => n + 1);
    } catch (e) {
      message.error(String(e));
    } finally {
      setResuming(false);
    }
  }

  async function onDiscardQueue() {
    if (resuming) return;
    try {
      await api.discardDownloadQueue();
      setPendingJobs([]);
      setResumeTick((n) => n + 1);
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
            resumeTick={resumeTick}
          />
        )}
        {tab === "library" && (
          <LibraryView categories={categories} onCategoriesChanged={refresh} />
        )}
        {tab === "settings" && <SettingsView onSaved={refresh} />}
      </Content>

      <Modal
        title="未完成的下载"
        open={pendingJobs.length > 0}
        okText="继续"
        cancelText="丢弃"
        onOk={onResumeQueue}
        onCancel={onDiscardQueue}
        confirmLoading={resuming}
        cancelButtonProps={{ disabled: resuming }}
        closable={false}
        maskClosable={false}
      >
        <p>{jobsSummary(pendingJobs)}</p>
      </Modal>
    </Layout>
  );
}

export default App;
