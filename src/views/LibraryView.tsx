import { useEffect, useState } from "react";
import {
  Alert,
  Button,
  Card,
  Empty,
  Flex,
  Input,
  Layout,
  Menu,
  Modal,
  Popconfirm,
  Select,
  Space,
  Table,
  Typography,
  message,
} from "antd";
import {
  DeleteOutlined,
  EditOutlined,
  FolderAddOutlined,
  PlayCircleOutlined,
} from "@ant-design/icons";
import { api } from "../api";
import type { VideoItem } from "../types";

const { Sider, Content } = Layout;

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
  const [loading, setLoading] = useState(false);
  const [newName, setNewName] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [moveTarget, setMoveTarget] = useState<Record<string, string>>({});
  const [renameOpen, setRenameOpen] = useState(false);
  const [renameValue, setRenameValue] = useState("");

  useEffect(() => {
    if (!categories.includes(active) && categories.length) {
      setActive(categories[0]);
    }
  }, [categories, active]);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      setLoading(true);
      try {
        const list = await api.listVideos(active);
        if (!cancelled) setVideos(list);
      } catch (e) {
        if (!cancelled) setError(String(e));
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [active]);

  async function refreshVideos() {
    setLoading(true);
    try {
      setVideos(await api.listVideos(active));
    } finally {
      setLoading(false);
    }
  }

  async function onCreate() {
    setError(null);
    try {
      await api.createCategory(newName.trim());
      setNewName("");
      message.success("分类已创建");
      onCategoriesChanged();
    } catch (e) {
      setError(String(e));
    }
  }

  async function onDeleteCategory() {
    if (active === "未分类") return;
    setError(null);
    try {
      await api.deleteCategory(active, true);
      message.success("分类已删除");
      onCategoriesChanged();
      setActive("未分类");
    } catch (e) {
      setError(String(e));
    }
  }

  async function onRenameCategory() {
    if (active === "未分类") return;
    const to = renameValue.trim();
    if (!to || to === active) {
      setRenameOpen(false);
      return;
    }
    setError(null);
    try {
      await api.renameCategory(active, to);
      message.success("分类已重命名");
      onCategoriesChanged();
      setActive(to);
      setRenameOpen(false);
    } catch (e) {
      setError(String(e));
    }
  }

  return (
    <Card title="视频库" bordered={false} className="page-card" styles={{ body: { padding: 0 } }}>
      <Layout className="library-layout">
        <Sider width={240} theme="light" className="library-sider">
          <Typography.Text type="secondary" className="sider-label">
            分类
          </Typography.Text>
          <Menu
            mode="inline"
            selectedKeys={[active]}
            onClick={({ key }) => setActive(key)}
            items={categories.map((c) => ({ key: c, label: c }))}
            style={{ border: 0, marginBottom: 12 }}
          />
          <Space direction="vertical" style={{ width: "100%", padding: "0 12px 16px" }}>
            <Input
              placeholder="新分类"
              value={newName}
              onChange={(e) => setNewName(e.target.value)}
              onPressEnter={onCreate}
              allowClear
            />
            <Button
              block
              type="dashed"
              icon={<FolderAddOutlined />}
              disabled={!newName.trim()}
              onClick={onCreate}
            >
              新建分类
            </Button>
            <Button
              block
              icon={<EditOutlined />}
              disabled={active === "未分类"}
              onClick={() => {
                setRenameValue(active);
                setRenameOpen(true);
              }}
            >
              重命名
            </Button>
            <Popconfirm
              title={`删除分类「${active}」？`}
              description="非空分类也会被强制删除。"
              okText="删除"
              cancelText="取消"
              okButtonProps={{ danger: true }}
              disabled={active === "未分类"}
              onConfirm={onDeleteCategory}
            >
              <Button
                block
                danger
                icon={<DeleteOutlined />}
                disabled={active === "未分类"}
              >
                删除分类
              </Button>
            </Popconfirm>
          </Space>
        </Sider>

        <Content className="library-content">
          <Flex justify="space-between" align="center" style={{ marginBottom: 12 }}>
            <Typography.Title level={5} style={{ margin: 0 }}>
              {active}
            </Typography.Title>
            <Typography.Text type="secondary">{videos.length} 个视频</Typography.Text>
          </Flex>

          {error && (
            <Alert
              type="error"
              showIcon
              closable
              message={error}
              onClose={() => setError(null)}
              style={{ marginBottom: 12 }}
            />
          )}

          <Table
            rowKey="path"
            loading={loading}
            dataSource={videos}
            pagination={videos.length > 8 ? { pageSize: 8 } : false}
            locale={{ emptyText: <Empty description="暂无视频" /> }}
            columns={[
              {
                title: "文件名",
                dataIndex: "name",
                ellipsis: true,
                render: (name: string) => (
                  <Typography.Text strong>{name}</Typography.Text>
                ),
              },
              {
                title: "大小",
                dataIndex: "size",
                width: 110,
                render: (size: number) => formatSize(size),
              },
              {
                title: "操作",
                key: "actions",
                width: 320,
                render: (_: unknown, v: VideoItem) => (
                  <Space wrap size="small">
                    <Button
                      size="small"
                      icon={<PlayCircleOutlined />}
                      onClick={() => api.openVideo(v.path)}
                    >
                      打开
                    </Button>
                    <Select
                      size="small"
                      placeholder="移动到…"
                      style={{ width: 120 }}
                      value={moveTarget[v.name] || undefined}
                      onChange={(value) =>
                        setMoveTarget((m) => ({ ...m, [v.name]: value }))
                      }
                      options={categories
                        .filter((c) => c !== active)
                        .map((c) => ({ value: c, label: c }))}
                      allowClear
                    />
                    <Button
                      size="small"
                      disabled={!moveTarget[v.name]}
                      onClick={async () => {
                        try {
                          await api.moveVideo(active, moveTarget[v.name], v.name);
                          message.success("已移动");
                          await refreshVideos();
                          onCategoriesChanged();
                        } catch (e) {
                          setError(String(e));
                        }
                      }}
                    >
                      移动
                    </Button>
                    <Popconfirm
                      title="删除这个视频？"
                      okText="删除"
                      cancelText="取消"
                      okButtonProps={{ danger: true }}
                      onConfirm={async () => {
                        try {
                          await api.deleteVideo(active, v.name);
                          message.success("已删除");
                          await refreshVideos();
                        } catch (e) {
                          setError(String(e));
                        }
                      }}
                    >
                      <Button size="small" danger icon={<DeleteOutlined />}>
                        删除
                      </Button>
                    </Popconfirm>
                  </Space>
                ),
              },
            ]}
          />
        </Content>
      </Layout>

      <Modal
        title="重命名分类"
        open={renameOpen}
        onOk={onRenameCategory}
        onCancel={() => setRenameOpen(false)}
        okText="确定"
        cancelText="取消"
      >
        <Input
          value={renameValue}
          onChange={(e) => setRenameValue(e.target.value)}
          onPressEnter={onRenameCategory}
          placeholder="新分类名"
        />
      </Modal>
    </Card>
  );
}
