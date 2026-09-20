import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  Alert,
  Button,
  Card,
  Checkbox,
  Col,
  Form,
  Input,
  Progress,
  Row,
  Select,
  Space,
  Typography,
} from "antd";
import { CloudDownloadOutlined, StopOutlined } from "@ant-design/icons";
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
  const [audioOnly, setAudioOnly] = useState(false);
  const [percent, setPercent] = useState<number | null>(null);
  const [logs, setLogs] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [donePath, setDonePath] = useState<string | null>(null);
  const logRef = useRef<HTMLPreElement>(null);
  const onCategoriesChangedRef = useRef(onCategoriesChanged);
  onCategoriesChangedRef.current = onCategoriesChanged;

  useEffect(() => {
    setCategory(defaultCategory);
  }, [defaultCategory]);

  useEffect(() => {
    setQuality(defaultQuality);
  }, [defaultQuality]);

  useEffect(() => {
    if (logRef.current) {
      logRef.current.scrollTop = logRef.current.scrollHeight;
    }
  }, [logs]);

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
        onCategoriesChangedRef.current();
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
      await api.startDownload(url.trim(), cat, quality, audioOnly);
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
    <Card title="下载视频" bordered={false} className="page-card">
      <Form layout="vertical" disabled={busy}>
        <Form.Item label="视频链接" required>
          <Input
            size="large"
            value={url}
            onChange={(e) => setUrl(e.target.value)}
            placeholder="粘贴 YouTube / Bilibili 链接"
            allowClear
          />
        </Form.Item>

        <Row gutter={16}>
          <Col xs={24} md={8}>
            <Form.Item label="分类">
              <Select
                size="large"
                value={category}
                onChange={setCategory}
                options={categories.map((c) => ({ value: c, label: c }))}
              />
            </Form.Item>
          </Col>
          <Col xs={24} md={8}>
            <Form.Item label="或新建分类">
              <Input
                size="large"
                value={newCategory}
                onChange={(e) => setNewCategory(e.target.value)}
                placeholder="新分类名"
                allowClear
              />
            </Form.Item>
          </Col>
          <Col xs={24} md={8}>
            <Form.Item label="清晰度">
              <Select
                size="large"
                value={quality}
                onChange={setQuality}
                disabled={audioOnly}
                options={[
                  { value: "720", label: "720p" },
                  { value: "1080", label: "1080p" },
                  { value: "best", label: "最高" },
                ]}
              />
            </Form.Item>
          </Col>
        </Row>

        <Form.Item style={{ marginBottom: 16 }}>
          <Checkbox
            checked={audioOnly}
            onChange={(e) => setAudioOnly(e.target.checked)}
          >
            仅下载音频 (MP3)
          </Checkbox>
        </Form.Item>
      </Form>

      <Space wrap style={{ marginBottom: 16 }}>
        <Button
          type="primary"
          size="large"
          icon={<CloudDownloadOutlined />}
          loading={busy}
          disabled={!url.trim()}
          onClick={onStart}
        >
          {busy ? "下载中…" : "开始下载"}
        </Button>
        <Button
          danger
          size="large"
          icon={<StopOutlined />}
          disabled={!busy}
          onClick={onStop}
        >
          停止
        </Button>
      </Space>

      {percent != null && (
        <Progress
          percent={Math.min(Number(percent.toFixed(1)), 100)}
          status={busy ? "active" : percent >= 100 ? "success" : "normal"}
          style={{ marginBottom: 16 }}
        />
      )}

      {error && (
        <Alert
          type="error"
          showIcon
          closable
          message={error}
          onClose={() => setError(null)}
          style={{ marginBottom: 16 }}
        />
      )}
      {donePath && (
        <Alert
          type="success"
          showIcon
          message="下载完成"
          description={
            <Typography.Text copyable ellipsis>
              {donePath}
            </Typography.Text>
          }
          style={{ marginBottom: 16 }}
        />
      )}

      <Typography.Text type="secondary">日志</Typography.Text>
      <pre className="log" ref={logRef}>
        {logs.length ? logs.join("\n") : "等待开始…"}
      </pre>
    </Card>
  );
}
