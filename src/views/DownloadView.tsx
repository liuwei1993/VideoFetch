import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  Alert,
  Button,
  Card,
  Checkbox,
  Col,
  Collapse,
  Form,
  Input,
  Progress,
  Row,
  Select,
  Space,
  Table,
  Typography,
} from "antd";
import { CloudDownloadOutlined, StopOutlined } from "@ant-design/icons";
import { api } from "../api";
import type {
  DownloadBatchFinished,
  DownloadBatchStarted,
  DownloadError,
  DownloadFinished,
  DownloadItemError,
  DownloadItemFinished,
  DownloadItemStarted,
  DownloadProgress,
} from "../types";

type TaskStatus = "pending" | "downloading" | "done" | "failed" | "cancelled";

type TaskRow = {
  index: number;
  id: string;
  title: string;
  status: TaskStatus;
  detail: string;
};

const statusLabel: Record<TaskStatus, string> = {
  pending: "等待",
  downloading: "下载中",
  done: "完成",
  failed: "失败",
  cancelled: "已取消",
};

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
  const [speed, setSpeed] = useState<string | null>(null);
  const [eta, setEta] = useState<string | null>(null);
  const [logs, setLogs] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [donePath, setDonePath] = useState<string | null>(null);
  const [tasks, setTasks] = useState<TaskRow[]>([]);
  const [batchMode, setBatchMode] = useState(false);
  const [batchResult, setBatchResult] = useState<DownloadBatchFinished | null>(
    null
  );
  const logRef = useRef<HTMLPreElement>(null);
  const onCategoriesChangedRef = useRef(onCategoriesChanged);
  onCategoriesChangedRef.current = onCategoriesChanged;
  const batchModeRef = useRef(false);
  batchModeRef.current = batchMode;

  const done = tasks.filter((t) => t.status === "done").length;
  const failed = tasks.filter((t) => t.status === "failed").length;
  const active = tasks.filter((t) => t.status === "downloading").length;
  const summaryText = batchMode
    ? `共 ${tasks.length} · 完成 ${done} · 失败 ${failed} · 进行中 ${active}`
    : null;

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
        if (e.payload.speed != null) setSpeed(e.payload.speed);
        if (e.payload.eta != null) setEta(e.payload.eta);
        setLogs((prev) => {
          const next = [...prev, e.payload.line];
          return next.length > 200 ? next.slice(-200) : next;
        });
      });
      const u2 = await listen<DownloadFinished>("download-finished", (e) => {
        if (cancelled) return;
        if (batchModeRef.current) return;
        setBusy(false);
        setDonePath(e.payload.path);
        setPercent(100);
        setSpeed(null);
        setEta(null);
        onCategoriesChangedRef.current();
      });
      const u3 = await listen<DownloadError>("download-error", (e) => {
        if (cancelled) return;
        if (batchModeRef.current) return;
        setBusy(false);
        setSpeed(null);
        setEta(null);
        if (e.payload.message.includes("已停止")) {
          setLogs((prev) => [...prev, e.payload.message]);
          setError(null);
        } else {
          setError(e.payload.message);
        }
      });
      const uBatchStart = await listen<DownloadBatchStarted>(
        "download-batch-started",
        (e) => {
          if (cancelled) return;
          setBatchMode(true);
          setBatchResult(null);
          setTasks(
            e.payload.items.map((it, index) => ({
              index,
              id: it.id,
              title: it.title,
              status: "pending" as const,
              detail: "",
            }))
          );
        }
      );
      const uItemStart = await listen<DownloadItemStarted>(
        "download-item-started",
        (e) => {
          if (cancelled) return;
          setTasks((prev) =>
            prev.map((t) =>
              t.index === e.payload.index
                ? { ...t, status: "downloading", detail: "" }
                : t
            )
          );
        }
      );
      const uItemDone = await listen<DownloadItemFinished>(
        "download-item-finished",
        (e) => {
          if (cancelled) return;
          setTasks((prev) =>
            prev.map((t) =>
              t.index === e.payload.index
                ? { ...t, status: "done", detail: e.payload.path }
                : t
            )
          );
        }
      );
      const uItemErr = await listen<DownloadItemError>(
        "download-item-error",
        (e) => {
          if (cancelled) return;
          setTasks((prev) =>
            prev.map((t) =>
              t.index === e.payload.index
                ? { ...t, status: "failed", detail: e.payload.message }
                : t
            )
          );
        }
      );
      const uBatchEnd = await listen<DownloadBatchFinished>(
        "download-batch-finished",
        (e) => {
          if (cancelled) return;
          setBusy(false);
          setSpeed(null);
          setEta(null);
          setTasks((prev) =>
            prev.map((t) =>
              t.status === "pending" || t.status === "downloading"
                ? { ...t, status: "cancelled", detail: t.detail || "已取消" }
                : t
            )
          );
          setBatchResult(e.payload);
          setPercent(100);
          onCategoriesChangedRef.current();
        }
      );
      if (cancelled) {
        u1();
        u2();
        u3();
        uBatchStart();
        uItemStart();
        uItemDone();
        uItemErr();
        uBatchEnd();
      } else {
        unsubs.push(
          u1,
          u2,
          u3,
          uBatchStart,
          uItemStart,
          uItemDone,
          uItemErr,
          uBatchEnd
        );
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
    setSpeed(null);
    setEta(null);
    setTasks([]);
    setBatchMode(false);
    setBatchResult(null);
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
            placeholder="粘贴 YouTube / Bilibili 链接（支持合集）"
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
          format={(p) => {
            const parts = [`${p}%`];
            if (busy && speed) parts.push(speed);
            if (busy && eta) parts.push(`剩余 ${eta}`);
            return parts.join(" · ");
          }}
          style={{ marginBottom: 16 }}
        />
      )}

      {batchMode && tasks.length > 0 && (
        <>
          <Typography.Paragraph type="secondary">
            {summaryText}
          </Typography.Paragraph>
          <Table
            size="small"
            pagination={false}
            rowKey="id"
            dataSource={tasks}
            scroll={{ y: 320 }}
            style={{ marginBottom: 16 }}
            columns={[
              {
                title: "#",
                dataIndex: "index",
                width: 56,
                render: (i: number) => i + 1,
              },
              { title: "标题", dataIndex: "title", ellipsis: true },
              {
                title: "状态",
                dataIndex: "status",
                width: 88,
                render: (s: TaskStatus) => statusLabel[s],
              },
              {
                title: "详情",
                dataIndex: "detail",
                ellipsis: true,
                render: (d: string, row: TaskRow) =>
                  row.status === "failed" ? (
                    <Typography.Text type="danger" ellipsis={{ tooltip: d }}>
                      {d}
                    </Typography.Text>
                  ) : row.status === "done" ? (
                    <Typography.Text
                      type="secondary"
                      ellipsis={{ tooltip: d }}
                    >
                      已保存
                    </Typography.Text>
                  ) : (
                    d
                  ),
              },
            ]}
          />
        </>
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
      {batchResult && (
        <Alert
          type={batchResult.failed === 0 ? "success" : "warning"}
          showIcon
          closable
          message={
            batchResult.failed === 0 ? "批量下载完成" : "批量下载结束"
          }
          description={`完成 ${batchResult.succeeded} · 失败 ${batchResult.failed} · 取消 ${batchResult.cancelled}`}
          onClose={() => setBatchResult(null)}
          style={{ marginBottom: 16 }}
        />
      )}
      {donePath && !batchMode && (
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

      <Collapse
        bordered={false}
        size="small"
        items={[
          {
            key: "logs",
            label: (
              <Typography.Text type="secondary">
                日志{logs.length ? `（${logs.length}）` : ""}
              </Typography.Text>
            ),
            children: (
              <pre className="log" ref={logRef}>
                {logs.length ? logs.join("\n") : "等待开始…"}
              </pre>
            ),
          },
        ]}
      />
    </Card>
  );
}
