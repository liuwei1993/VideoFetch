# 下载速度与预计剩余时间设计

日期：2026-09-20  
状态：已口头批准，待书面确认后实现

## 目标

下载进行中时，在进度条百分比旁显示当前下载速度和预计剩余时间。

## UI

- 位置：Ant Design `Progress` 的百分比文案旁（用户选定方案 2）
- 下载中完整格式：`45% · 7.5 MB/s · 剩余 1:25`
- 仅有速度：`45% · 7.5 MB/s`
- 仅有百分比：`45%`
- 完成：`100%`
- 合并/转码等暂无速度行：保留上一次有效的速度与 ETA；若从未解析到则只显示百分比
- 任务结束（完成 / 失败 / 停止）或开始新任务时清空速度与 ETA

## 数据流

1. yt-dlp 进度行（已有 `--newline --progress`）形如：  
   `[download]  45.2% of  237.23MiB at    7.54MiB/s ETA 00:25`
2. 后端在现有 `parse_percent` 旁解析 `speed`、`eta`
3. 经 `download-progress` 事件发给前端
4. `DownloadView` 用 `Progress` 的 `format` 拼出上述文案

## API / 类型

扩展 `DownloadProgress`（Rust `Serialize` + TS）：

| 字段 | 类型 | 说明 |
|------|------|------|
| `percent` | `number \| null` | 已有 |
| `line` | `string` | 已有，完整日志行 |
| `speed` | `string \| null` | 展示用，如 `"7.5 MB/s"` |
| `eta` | `string \| null` | 展示用，如 `"1:25"` 或 `"1:02:03"` |

事件名与其它字段不变；旧前端忽略新字段亦可。

## 解析规则

- **速度**：匹配 `at` 与 `/s` 之间的片段；将 `KiB/s`/`MiB/s`/`GiB/s` 规范为 `KB/s`/`MB/s`/`GB/s`；数值保留一位小数（如 `7.5 MB/s`）
- **ETA**：匹配 `ETA` 后的 `HH:MM:SS` 或 `MM:SS`；展示为去掉无意义前导零小时的时钟风格（`00:25` → `0:25`，`01:02:03` → `1:02:03`）；未知/不可用（如 `Unknown`、`--:--`）视为 `null`
- 非 `[download]` 进度行：`speed`/`eta` 为 `null`（前端保留上次值）
- 单元测试覆盖：现有 percent 样例行、无速度行、未知 ETA

## 前端状态

- `speed` / `eta`：`string | null`，仅在 payload 对应字段非 null 时更新
- `onStart` 时重置为 `null`；`download-finished` / `download-error` 时清空（或完成时只保留 `100%`）

## 非目标

- 不自行按 `.part` 字节估算速度/ETA
- 不做平均速度历史或图表
- 不改库、设置、停止、音频模式逻辑
- 不在日志折叠区额外高亮速度行

## 测试

- Rust：`parse_speed` / `parse_eta`（或合并解析函数）单元测试
- 手动：开始下载后进度条旁出现速度与剩余时间；完成/停止后恢复为仅百分比或成功态
