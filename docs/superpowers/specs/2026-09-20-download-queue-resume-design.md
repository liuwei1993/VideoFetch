# 下载队列持久化与恢复设计

日期：2026-09-20  
状态：已批准（对话确认）

## 目标

重启或意外退出后，能恢复未完成的下载（合集批量 + 单视频）。启动时弹窗询问「继续 / 丢弃」；合集中失败项在继续时一并重试；半成品通过 yt-dlp `-c` 续传。

## 交互结论（已确认）

| 项 | 选择 |
|---|---|
| 范围 | 合集 + 单视频 |
| 启动 | Modal：继续 / 丢弃（不自动开下） |
| 失败项 | 继续时一并重试 |
| 实现 | 队列快照 JSON（`download_queue.json`） |

## 持久化模型

**路径：** 应用 config 目录下的 `download_queue.json`（与 `settings.json` 同级）。

**约束：** 同一时间只保留一个活跃会话（与现有「同时只能一个下载会话」一致）。

**结构：**

```json
{
  "version": 1,
  "kind": "batch",
  "page_url": "https://space.bilibili.com/.../lists/...?type=season",
  "category": "脱口秀",
  "quality": "720",
  "audio_only": false,
  "updated_at": "2026-09-20T12:00:00Z",
  "items": [
    {
      "index": 0,
      "id": "BVxxx",
      "title": "第一集",
      "url": "https://www.bilibili.com/video/BVxxx",
      "status": "done"
    },
    {
      "index": 1,
      "id": "BVyyy",
      "title": "第二集",
      "url": "https://www.bilibili.com/video/BVyyy",
      "status": "downloading"
    }
  ]
}
```

| 字段 | 说明 |
|---|---|
| `kind` | `batch` \| `single` |
| `page_url` | 用户粘贴的原始 URL（合集页或单视频 URL） |
| `items[].url` | 实际下载 URL；单视频时与 `page_url` 相同 |
| `status` | `pending` \| `downloading` \| `done` \| `failed` |

**不持久化** `cancelled`：用户点「停止」视为放弃，删除队列文件。

### 写入时机

| 时机 | 行为 |
|---|---|
| 会话开始 | 写出完整队列（合集：展开后立刻写） |
| 条目状态变化 | 更新为 `downloading` / `done` / `failed` 并落盘 |
| 全部完成 | 删除队列文件 |
| 用户停止 | 删除队列文件 |
| 用户丢弃 | 删除队列文件 |

### 可恢复判定

存在队列文件，且至少有一条 `pending` / `downloading` / `failed` → 启动时弹窗。  
全 `done` 或文件缺失 / JSON 损坏 → 不弹（损坏时当作无队列，可打日志）。

## 启动弹窗

1. App 启动（或下载页首次 mount）调用 `get_download_queue`。  
2. 可恢复时 Modal：  
   - 标题：未完成的下载  
   - 正文：合集 →「合集 · 共 N · 已完成 x · 待处理 y」；单视频 → 标题/URL 摘要 + 分类  
   - 按钮：**继续** | **丢弃**  
3. 无队列 → 不弹。

### 继续

- 切到下载页，重建任务表：`done` 保持完成；`pending` / `downloading` / `failed` 视为待调度并重试。  
- 使用快照中的 `category` / `quality` / `audio_only`（不改用当前设置默认值）。  
- 并发数仍读**当前**设置中的 `max_concurrent_downloads`。  
- 只调度非 `done` 条目；yt-dlp 一律 `--continue`（`-c`）。  
- 已完成成品一般由 yt-dlp 跳过或快速结束。

### 丢弃

- 删除 `download_queue.json`。  
- 清理该会话相关的 `.part` / `.ytdl` 等临时文件（尽量按分类目录与已知 id/文件名模式清理）。  
- **不**删除已下好的成品。  
- 不开始下载。

### 与「停止」的关系

| 操作 | 队列 |
|---|---|
| 停止 | 删除（下次不提示） |
| 崩溃 / 杀进程 / 关窗（未点停止） | 保留 → 下次可恢复 |

## yt-dlp 续传

所有 `run_download`（单视频 + 合集条目）增加 `--continue`（`-c`）。  
输出模板与落盘路径不变。

## API

| Command | 作用 |
|---|---|
| `get_download_queue` | 读队列；无可恢复返回 `null` |
| `resume_download_queue` | 「继续」：按快照恢复会话 |
| `discard_download_queue` | 「丢弃」：删队列 + 清临时文件 |

现有 `start_download` / `stop_download` 保留：

- `start_download`：创建或覆盖队列  
- `stop_download`：停止进程并删除队列  

恢复过程占用现有全局下载锁；恢复中禁止再开新下载。

## 前端

- 启动时拉队列 → 有则 Modal。  
- 「继续」后走现有事件流（合集：`download-batch-*`；单视频：`download-progress` / `download-finished`）。  
- 「丢弃」后关闭 Modal，下载页可空闲。

## 边界情况

| 场景 | 行为 |
|---|---|
| 恢复时分类目录已删 | 自动重建分类后继续 |
| 队列 JSON 损坏 | 当作无队列 |
| 「继续」后再次崩溃 | 队列持续更新，下次再提示 |
| 新任务开始时已有队列 | 覆盖为新会话 |
| 清晰度 / 仅音频 | 以快照为准 |
| 并发数 | 以当前设置为准 |

## 验证标准

1. 合集下到一半强制杀进程 → 重启弹窗 → 继续 → 已完成跳过，其余继续。  
2. 单视频下到一半杀进程 → 重启继续 → `-c` 续传。  
3. 点丢弃 → 不再提示；临时文件被清。  
4. 点停止 → 下次不提示。  
5. 失败项在「继续」后会重试。  
6. 新开始下载会覆盖旧队列。

## 本期不做

- 多会话队列 / 历史任务列表  
- 云同步  
- 条目内精确百分比断点 UI（只恢复条目级状态）  
- 停止后仍保留「可恢复」选项  

## 风险

- `.part` 清理需谨慎，避免误删无关文件（按会话 id / 分类目录限定）。  
- 已 `done` 但成品被用户手动删除：继续时该条可能被 yt-dlp 重新完整下载（可接受）。  
- 关窗若先于落盘：极短窗口内状态可能落后一条；以最后一次成功写入为准。
