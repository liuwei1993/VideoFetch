# Bilibili 合集批量下载设计

日期：2026-09-20  
状态：已批准（对话确认）

## 目标

支持粘贴 Bilibili 视频合集链接（如 `https://space.bilibili.com/.../lists/...?type=season`），展开后批量下载；下载页展示类迅雷任务列表；失败项单独标记并继续其余任务。同时下载数可在设置中配置，默认 5。

## 背景

- 现有下载固定 `--no-playlist`，合集 URL 只会下第一条或失败。
- yt-dlp 已能解析该合集：`--flat-playlist` 可列出全部 BV id。
- 原设计「第一版明确不做播放列表 / 批量队列 UI」；本期显式放开合集批量。

## 交互结论（已确认）

| 项 | 选择 |
|---|---|
| 触发 | 粘贴合集 URL → 开始即整批下载（不先多选） |
| 失败 | 标记该条失败，继续队列（类迅雷） |
| UI | 任务列表：每集一行状态（等待 / 下载中 / 完成 / 失败 / 已取消） |
| 并发 | 设置可配，默认 5；`1` = 串行 |

## 总体流程

1. 用户粘贴 URL，选择分类 / 清晰度 / 仅音频，点「开始下载」。
2. 后端判断是否为合集 URL。
3. **合集：** `--flat-playlist` 展开 → 发 `download-batch-started` → 有限并发队列逐条下载。
4. **单视频：** 行为与现在一致（仍 `--no-playlist`）。
5. 每条：`pending` → `downloading` → `done` / `failed`；失败记原因，不中断队列。
6. 「停止」：杀掉全部活跃 yt-dlp，剩余 `pending` → `cancelled`。
7. 整批结束发 `download-batch-finished`，释放全局下载锁。

范围：本期以 Bilibili 合集为主；若同一展开逻辑天然覆盖其它 playlist，可顺带兼容，但不单独做 YouTube playlist 识别。

## 合集识别

URL（大小写不敏感）满足任一即视为合集：

- `space.bilibili.com/.../lists/<id>`（含 `type=season` / `type=series` 等）
- 其它明确的 Bilibili 合集 / 播单形态（实现时可按 path 规则扩展）

**不当合集：** `bilibili.com/video/BVxxx`、短链 `b23.tv`（解析后若落到单视频页，按单视频处理）。单视频始终 `--no-playlist`。

## 展开

```text
yt-dlp --flat-playlist --print "%(id)s\t%(title)s" <合集URL>
```

- 有序输出；空列表或命令失败 → 整批无法启动，发 `download-error`。
- 每条下载 URL：`https://www.bilibili.com/video/<id>`。
- 代理：与现有 Bilibili 策略一致（默认直连；设置开启则走代理）。

## 落盘

- 目录：用户所选分类 `~/videofetch/<分类>/`（库根以设置为准）。
- 文件名：`%(title)s [%(id)s].%(ext)s`（与单视频相同）。
- **不**自动创建合集子文件夹。
- 清晰度 / 仅音频：整批共用开始时的选项。
- 已存在文件：与现有单视频 yt-dlp 行为保持一致。

## 并发与进程

- 设置字段：`max_concurrent_downloads`（整数，默认 **5**，建议 UI 限制 1–10）。
- 合集：最多 N 个并行 yt-dlp；某条结束后再取下一条 pending。
- 单视频：始终 1 个进程。
- 全局锁：同时只允许一个「下载会话」（单视频或一整批合集）；合集占用至 `batch-finished`。
- 停止：kill 所有活跃子进程（含 process group / ffmpeg），pending 标 `cancelled`。

## 下载页 UI

### 单视频

保持现有：进度条（含速度 / ETA）+ 成功 / 失败 Alert + 日志。

### 合集

- 顶部摘要：`共 N · 完成 x · 失败 y · 进行中 k`
- 现有 `Progress`：表示**当前活跃任务中**代表性进度（例如进度最高的一集，或任一活跃任务）；任务结束清空速度 / ETA。
- 任务表（Ant Design `Table`）：

| 列 | 内容 |
|---|---|
| # | 序号（从 1） |
| 标题 | flat 解析标题；缺失则用 id |
| 状态 | 等待 / 下载中 / 完成 / 失败 / 已取消 |
| 详情 | 完成：可显示「已保存」或路径缩略；失败：短错误，hover 全文 |

本期不做：行内迷你进度条、失败重试按钮、任务持久化（重启恢复）。

## 事件协议

| 事件 | Payload（示意） | 说明 |
|---|---|---|
| `download-batch-started` | `{ total, items: [{ id, title }] }` | 建表 |
| `download-item-started` | `{ index, id }` | 行 → downloading |
| `download-progress` | 现有结构 | 仅描述某一活跃集；可带 `id`/`index` 便于扩展（可选） |
| `download-item-finished` | `{ index, id, path }` | 行 → done |
| `download-item-error` | `{ index, id, message }` | 行 → failed |
| `download-batch-finished` | `{ succeeded, failed, cancelled }` | 整批结束，`busy=false` |
| `download-finished` | `{ path }` | **仅单视频**完成 |
| `download-error` | `{ message }` | 单视频失败，或合集**无法启动**（解析失败等） |

前端：若收到 `download-batch-started`，走批量状态机；否则走原单视频逻辑。

## API / 设置

- `start_download({ url, category, quality, audioOnly })`：入参不变；后端内部分支合集 / 单视频。
- `stop_download`：行为扩展为停掉会话内全部活跃进程 + 取消队列。
- `Settings` 新增：`max_concurrent_downloads: number`（默认 5）。
- 设置页：数字输入「同时下载数」，校验 1–10。

## 错误与停止

| 场景 | 行为 |
|---|---|
| 合集解析失败 / 0 条 | `download-error`，不进入有效批量下载 |
| 单集失败 | 该行 `failed` + 原因，队列继续 |
| 用户停止 | 活跃中断；pending → `cancelled`；已完成 / 已失败保留 |
| 整批结束且有失败 | 仍发 `download-batch-finished`；UI 用摘要提示，不整批 `download-error` |

## 验证标准

1. 粘贴示例合集 URL，出现任务表且条目数与 flat 列表一致。
2. 默认并发 5：最多同时约 5 行「下载中」。
3. 人为使一集失败 → 该行失败，其余继续，最终计数正确。
4. 下载中点停止 → 活跃中断、pending 取消。
5. 单视频链接行为与改前一致。
6. 设置改为 1 后，合集严格串行。
7. 改并发设置并重启后仍生效（写入 `settings.json`）。

## 本期不做

- 合集子目录选项
- 失败重试 / 只下失败项
- 任务列表持久化与重启恢复
- YouTube playlist 专用产品化（可选技术兼容）
- 行内精确多进度条（可后续加 `id` 到 progress 事件）

## 风险

- 并发过高可能触发 Bilibili 限流；默认 5、上限 10 缓解。
- flat 标题偶发 `NA`：UI 回退显示 id。
- 多进程进度事件交错：进度条取代表性一集即可；状态以 item 事件为准。
