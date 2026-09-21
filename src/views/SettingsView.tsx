import { useEffect, useState } from "react";
import {
  Alert,
  Button,
  Card,
  Form,
  Input,
  InputNumber,
  Select,
  Spin,
  Switch,
  Typography,
  message,
} from "antd";
import { SaveOutlined } from "@ant-design/icons";
import { api } from "../api";
import type { Settings } from "../types";

type Props = {
  onSaved: () => void;
};

export function SettingsView({ onSaved }: Props) {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.getSettings().then(setSettings).catch((e) => setError(String(e)));
  }, []);

  async function onSave() {
    if (!settings) return;
    setError(null);
    setSaving(true);
    try {
      await api.saveSettings(settings);
      message.success("设置已保存");
      onSaved();
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  }

  if (!settings) {
    return (
      <Card title="设置" bordered={false} className="page-card">
        {error ? (
          <Alert type="error" showIcon message={error} />
        ) : (
          <Spin tip="加载中…" />
        )}
      </Card>
    );
  }

  return (
    <Card title="设置" bordered={false} className="page-card">
      <Form layout="vertical" style={{ maxWidth: 560 }}>
        <Form.Item label="库根目录" extra="支持 ~ 表示用户家目录">
          <Input
            value={settings.library_root}
            onChange={(e) =>
              setSettings({ ...settings, library_root: e.target.value })
            }
          />
        </Form.Item>

        <Form.Item label="默认清晰度">
          <Select
            value={settings.default_quality}
            onChange={(value) =>
              setSettings({ ...settings, default_quality: value })
            }
            options={[
              { value: "720", label: "720p" },
              { value: "1080", label: "1080p" },
              { value: "best", label: "最高" },
            ]}
          />
        </Form.Item>

        <Form.Item label="YouTube 代理" extra="YouTube / MissAV 使用此代理；Bilibili 默认直连，可在下方单独开启">
          <Input
            value={settings.youtube_proxy}
            onChange={(e) =>
              setSettings({ ...settings, youtube_proxy: e.target.value })
            }
            placeholder="http://127.0.0.1:57890"
          />
        </Form.Item>

        <Form.Item label="Bilibili 也走代理">
          <Switch
            checked={settings.bilibili_use_proxy}
            onChange={(checked) =>
              setSettings({ ...settings, bilibili_use_proxy: checked })
            }
          />
        </Form.Item>

        <Form.Item
          label="同时下载数"
          extra="所有任务合计最多并行几个 yt-dlp 进程（1–10，默认 5）"
        >
          <InputNumber
            min={1}
            max={10}
            value={settings.max_concurrent_downloads}
            onChange={(v) =>
              setSettings({
                ...settings,
                max_concurrent_downloads: typeof v === "number" ? v : 5,
              })
            }
          />
        </Form.Item>

        <Form.Item label="Cookie 文件" extra="预留项，当前下载流程暂不接入">
          <Input
            value={settings.cookie_file ?? ""}
            onChange={(e) =>
              setSettings({
                ...settings,
                cookie_file: e.target.value.trim() ? e.target.value : null,
              })
            }
            placeholder="可选路径"
          />
        </Form.Item>

        <Typography.Paragraph type="secondary">
          上次分类：{settings.last_category || "未分类"}
        </Typography.Paragraph>

        {error && (
          <Alert
            type="error"
            showIcon
            message={error}
            style={{ marginBottom: 16 }}
          />
        )}

        <Button
          type="primary"
          icon={<SaveOutlined />}
          loading={saving}
          onClick={onSave}
        >
          保存
        </Button>
      </Form>
    </Card>
  );
}
