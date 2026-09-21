import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  Button,
  Card,
  Checkbox,
  Col,
  Empty,
  Form,
  Input,
  Modal,
  Progress,
  Row,
  Select,
  Space,
  Tag,
  Typography,
  message,
} from "antd";
import {
  CloudDownloadOutlined,
  PlusOutlined,
  StopOutlined,
} from "@ant-design/icons";
import { api } from "../api";
import type {
  DownloadError,
  DownloadFinished,
  DownloadJob,
  DownloadProgress,
  JobStatus,
} from "../types";

const statusMeta: Record<JobStatus, { label: string; color: string }> = {
  pending: { label: "等待中", color: "default" },
  downloading: { label: "下载中", color: "processing" },
  done: { label: "完成", color: "success" },
  failed: { label: "失败", color: "error" },
  cancelled: { label: "已取消", color: "warning" },
};

const qualityLabel: Record<string, string> = {
  "720": "720p",
  "1080": "1080p",
  best: "最高",
};

type Props = {
  categories: string[];
  defaultCategory: string;
  defaultQuality: string;
  onCategoriesChanged: () => void;
  resumeTick: number;
};

function upsertJob(prev: DownloadJob[], job: DownloadJob): DownloadJob[] {
  const i = prev.findIndex((j) => j.id === job.id);
  if (i === -1) return [job, ...prev];
  const next = prev.slice();
  next[i] = { ...next[i], ...job };
  return next;
}

function patchJob(
  prev: DownloadJob[],
  id: string,
  patch: Partial<DownloadJob>,
): DownloadJob[] {
  return prev.map((j) => (j.id === id ? { ...j, ...patch } : j));
}

function progressStatus(job: DownloadJob): "active" | "success" | "exception" | "normal" {
  if (job.status === "downloading") return "active";
  if (job.status === "done") return "success";
  if (job.status === "failed") return "exception";
  return "normal";
}

function formatProgress(job: DownloadJob, percent: number): string {
  const parts = [`${percent}%`];
  if (job.kind === "batch" && job.total > 1) {
    parts.unshift(`${job.completed}/${job.total}`);
  }
  if (job.status === "downloading" && job.speed) parts.push(job.speed);
  if (job.status === "downloading" && job.eta) parts.push(`剩余 ${job.eta}`);
  return parts.join(" · ");
}

export function DownloadView({
  categories,
  defaultCategory,
  defaultQuality,
  onCategoriesChanged,
  resumeTick,
}: Props) {
  const [jobs, setJobs] = useState<DownloadJob[]>([]);
  const [modalOpen, setModalOpen] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [url, setUrl] = useState("");
  const [category, setCategory] = useState(defaultCategory);
  const [newCategory, setNewCategory] = useState("");
  const [quality, setQuality] = useState(defaultQuality);
  const [audioOnly, setAudioOnly] = useState(false);
  const onCategoriesChangedRef = useRef(onCategoriesChanged);
  onCategoriesChangedRef.current = onCategoriesChanged;

  const activeCount = jobs.filter(
    (j) => j.status === "pending" || j.status === "downloading",
  ).length;

  useEffect(() => {
    setCategory(defaultCategory);
  }, [defaultCategory]);

  useEffect(() => {
    setQuality(defaultQuality);
  }, [defaultQuality]);

  useEffect(() => {
    api
      .listDownloadJobs()
      .then(setJobs)
      .catch((e) => message.error(String(e)));
  }, [resumeTick]);

  useEffect(() => {
    let cancelled = false;
    const unsubs: Array<() => void> = [];

    (async () => {
      const uUpsert = await listen<DownloadJob>("download-job-upsert", (e) => {
        if (cancelled) return;
        setJobs((prev) => upsertJob(prev, e.payload));
        if (
          e.payload.status === "done" ||
          e.payload.status === "failed" ||
          e.payload.status === "cancelled"
        ) {
          onCategoriesChangedRef.current();
        }
      });
      const uProgress = await listen<DownloadProgress>(
        "download-progress",
        (e) => {
          if (cancelled) return;
          const { jobId, percent, line, speed, eta } = e.payload;
          setJobs((prev) =>
            patchJob(prev, jobId, {
              detail: line,
              ...(percent != null ? { percent } : {}),
              ...(speed != null ? { speed } : {}),
              ...(eta != null ? { eta } : {}),
            }),
          );
        },
      );
      const uDone = await listen<DownloadFinished>("download-finished", (e) => {
        if (cancelled) return;
        setJobs((prev) =>
          patchJob(prev, e.payload.jobId, {
            status: "done",
            path: e.payload.path,
            percent: 100,
            speed: null,
            eta: null,
            detail: "已保存",
          }),
        );
        onCategoriesChangedRef.current();
      });
      const uErr = await listen<DownloadError>("download-error", (e) => {
        if (cancelled) return;
        const stopped = e.payload.message.includes("已停止");
        setJobs((prev) =>
          patchJob(prev, e.payload.jobId, {
            status: stopped ? "cancelled" : "failed",
            error: stopped ? null : e.payload.message,
            speed: null,
            eta: null,
            detail: e.payload.message,
          }),
        );
      });
      if (cancelled) {
        uUpsert();
        uProgress();
        uDone();
        uErr();
      } else {
        unsubs.push(uUpsert, uProgress, uDone, uErr);
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

  async function onCreate() {
    if (!url.trim()) {
      message.warning("请输入视频链接");
      return;
    }
    setSubmitting(true);
    try {
      const cat = await ensureCategorySelected();
      await api.startDownload(url.trim(), cat, quality, audioOnly);
      const listed = await api.listDownloadJobs();
      setJobs(listed);
      setModalOpen(false);
      setUrl("");
      setAudioOnly(false);
      setQuality(defaultQuality);
    } catch (e) {
      message.error(String(e));
    } finally {
      setSubmitting(false);
    }
  }

  async function onStop(jobId: string) {
    try {
      await api.stopDownload(jobId);
    } catch (e) {
      message.error(String(e));
    }
  }

  async function onStopAll() {
    try {
      await api.stopAllDownloads();
    } catch (e) {
      message.error(String(e));
    }
  }

  return (
    <Card
      title="下载任务"
      bordered={false}
      className="page-card"
      extra={
        <Space>
          {activeCount > 0 && (
            <Button danger icon={<StopOutlined />} onClick={onStopAll}>
              全部停止
            </Button>
          )}
          <Button
            type="primary"
            icon={<PlusOutlined />}
            onClick={() => setModalOpen(true)}
          >
            新建下载
          </Button>
        </Space>
      }
    >
      {jobs.length === 0 ? (
        <Empty
          image={Empty.PRESENTED_IMAGE_SIMPLE}
          description="还没有下载任务，点击右上角「新建下载」开始"
        />
      ) : (
        <div className="job-list">
          {jobs.map((job) => {
            const percent = Math.min(
              Number((job.percent ?? 0).toFixed(1)),
              100,
            );
            const meta = statusMeta[job.status];
            return (
              <div className="job-row" key={job.id}>
                <div className="job-row-head">
                  <Typography.Text strong ellipsis={{ tooltip: job.title }}>
                    {job.title || job.url}
                  </Typography.Text>
                  <Space size={8} wrap>
                    <Tag color={meta.color}>{meta.label}</Tag>
                    <Tag>
                      {job.audioOnly
                        ? "音频 MP3"
                        : qualityLabel[job.quality] || job.quality}
                    </Tag>
                    {job.category ? <Tag>{job.category}</Tag> : null}
                    {(job.status === "pending" ||
                      job.status === "downloading") && (
                      <Button
                        size="small"
                        danger
                        icon={<StopOutlined />}
                        onClick={() => onStop(job.id)}
                      >
                        停止
                      </Button>
                    )}
                  </Space>
                </div>
                <Typography.Text type="secondary" ellipsis={{ tooltip: job.url }}>
                  {job.url}
                </Typography.Text>
                <Progress
                  percent={percent}
                  status={progressStatus(job)}
                  format={() => formatProgress(job, percent)}
                />
                {job.detail ? (
                  <Typography.Text
                    type={job.status === "failed" ? "danger" : "secondary"}
                    ellipsis={{ tooltip: job.detail }}
                  >
                    {job.path || job.detail}
                  </Typography.Text>
                ) : null}
              </div>
            );
          })}
        </div>
      )}

      <Modal
        title="新建下载"
        open={modalOpen}
        okText="开始下载"
        cancelText="取消"
        confirmLoading={submitting}
        okButtonProps={{
          icon: <CloudDownloadOutlined />,
          disabled: !url.trim(),
        }}
        onOk={onCreate}
        onCancel={() => {
          if (!submitting) setModalOpen(false);
        }}
        destroyOnHidden
      >
        <Form layout="vertical" style={{ marginTop: 8 }}>
          <Form.Item label="视频链接" required>
            <Input
              size="large"
              value={url}
              onChange={(e) => setUrl(e.target.value)}
              placeholder="粘贴 YouTube / Bilibili / MissAV 链接（支持频道、播放列表与合集）"
              allowClear
              autoFocus
            />
          </Form.Item>
          <Row gutter={16}>
            <Col xs={24} md={12}>
              <Form.Item label="分类">
                <Select
                  size="large"
                  value={category}
                  onChange={setCategory}
                  options={categories.map((c) => ({ value: c, label: c }))}
                />
              </Form.Item>
            </Col>
            <Col xs={24} md={12}>
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
          </Row>
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
          <Form.Item>
            <Checkbox
              checked={audioOnly}
              onChange={(e) => setAudioOnly(e.target.checked)}
            >
              仅下载音频 (MP3)
            </Checkbox>
          </Form.Item>
        </Form>
      </Modal>
    </Card>
  );
}
