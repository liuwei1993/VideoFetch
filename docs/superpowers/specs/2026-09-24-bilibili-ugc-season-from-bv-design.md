# Bilibili 视频页 ugc_season 整部合集下载设计

日期：2026-09-24  
状态：待用户确认 spec

## 目标

粘贴属于 B 站「合集」（`ugc_season`）的视频页链接（如 `https://www.bilibili.com/video/BV1NCgVzoEG9/`）时，自动下载**整部合集**的全部稿件与分 P，并按合集 → 稿件分层落盘。

示例合集：「AI入门」（两门课，约 17 个分 P）。

## 背景

- 现有合集批量仅识别 `space.bilibili.com/.../lists/<id>`；`bilibili.com/video/BVxxx` 走单视频，且固定 `--no-playlist`，只会下第一 P。
- 用户常见习惯是复制视频页链接，而不是空间合集页。
- 该 BV 的 view API 含 `ugc_season`；侧栏「完整合集」条目对应稿件下的 `pages[]`（分 P），不是独立 BV。

## 交互结论（已确认）

| 项 | 选择 |
|---|---|
| 范围 | 整部 `ugc_season`（两门课全部集 P），不是只下当前稿 |
| 触发 | 粘贴 BV 视频页；有 `ugc_season` 则整批下载 |
| 无合集的 BV | 保持现有单视频行为（本期不自动展开纯分 P） |
| 落盘 | `{分类}/{合集标题}/{稿件标题}/` 下各分 P |
| 方案 | A：BV 自动展开 ugc_season |

## 识别与展开

### 触发

URL 为 Bilibili 视频页（`bilibili.com/video/BVxxx`，含查询参数；短链解析后若落到视频页同理）时：

1. 请求 `https://api.bilibili.com/x/web-interface/view?bvid=<BV>`。
2. 若 `data.ugc_season` 存在 → **整部合集批量下载**。
3. 若不存在 → **现有单视频路径**（仍 `--no-playlist`）。

空间页 `space.bilibili.com/.../lists/...?type=season|series` 仍走现有 `is_bilibili_collection_url` + yt-dlp `--flat-playlist`，本期不重做。

### 展开规则（有 `ugc_season`）

- 遍历 `ugc_season.sections[].episodes[]`。
- 每个 episode：取其全部 `pages[]`（若缺省则视为单 P）。
- 每条任务：
  - URL：`https://www.bilibili.com/video/{bvid}?p={page}`
  - 展示标题：优先 `pages[].part`，否则稿件标题 / bvid
  - 稳定 id：建议 `{bvid}_p{page}`（任务表、队列、文件名后缀一致）
- 有序：按 section → episode → page 顺序。
- view API 失败或合集条目为 0 → `download-error`，不启动批量。

对 `BV1NCgVzoEG9` 预期约 **17** 条（8 + 9）。

## 落盘

```text
{库根}/{分类}/{合集标题}/{稿件标题}/{分P标题} [{bvid}_p{N}].{ext}
```

示例：

```text
未分类/AI入门/【完整合集】一小时从函数到Transformer！/01 从函数到神经网络 [BV1NCgVzoEG9_p1].mp4
未分类/AI入门/【闪客】一小时从 Transformer 到大模型！/00 没什么用的前言 [BV15z4C6SEHT_p1].mp4
```

- **合集标题** ← `ugc_season.title`
- **稿件标题** ← episode 标题（如 `title` / 合集内显示名）
- **分 P 标题** ← `pages[].part`
- 三级目录名与文件名均做安全化（去掉 `/ \ : * ? " < > |` 及控制字符；过长截断）
- 清晰度 / 仅音频：整批共用开始时的选项
- 已存在文件：沿用 yt-dlp `--continue`
- 实现上可为每条任务传入「相对分类的子目录」给现有 `run_download`，避免改全局输出模板语义

## 并发与进程

- 复用现有合集批量：worker 池 + `max_concurrent_downloads`（默认 5，设置 1–10）
- 全局仍同时只允许一个下载会话
- 「停止」：杀掉全部活跃 yt-dlp，pending → cancelled
- 单条失败：该行 failed，队列继续；整批发 `download-batch-finished`

## UI / 事件

- 复用现有合集任务表与事件：`download-batch-started` / `item-started|finished|error` / `batch-finished`
- 任务行标题显示分 P 名；实现方便时可在详情带稿件名（非必须）
- 前端入参不变：`start_download({ url, category, quality, audioOnly })`；后端识别分支

## 与现有逻辑关系

| URL 类型 | 行为 |
|---|---|
| `space.../lists/...` | 现有 flat-playlist 批量（目录仍为所选分类根下，本期不强制套 ugc 分层） |
| `video/BV` + `ugc_season` | **本期新增**：API 展开全部分 P + 分层目录 |
| `video/BV` 无合集 | 现有单视频 |
| YouTube / MissAV | 不变 |

`site::is_batch_url` 可扩展为：空间合集 **或**（实现时）在解析阶段确认有 ugc_season 的 BV；若解析异步，也可在 `start_download` 内先 probe 再分支 batch/single。

## 本期不做

- 无 `ugc_season` 的多分 P 自动全下
- 「只下当前稿 / 整部合集」用户开关
- 空间合集页也强制套 `{合集}/{稿件}/` 分层（可后续统一）
- YouTube / MissAV 行为变更
- 失败重试按钮、行内多进度条

## 验证标准

1. 粘贴 `https://www.bilibili.com/video/BV1NCgVzoEG9/` → 任务约 17 条，两门课分 P 齐全。
2. 磁盘出现 `{分类}/AI入门/<课1>/`、`{分类}/AI入门/<课2>/`，文件名含 `[BVxxx_pN]`。
3. 无合集的普通 BV → 行为与改前一致。
4. 下载中点停止 → 活跃中断、pending 取消。
5. view API 失败 → 明确错误，不静默只下第一 P。

## 风险

- 挂在合集上的 BV 会整部下载，可能超出「只想下一集」的预期（已确认接受方案 A）。
- B 站 API / 限流：并发沿用现有上限；必要时可后续对 B 站单独降并发。
- 标题含特殊字符：安全化后可能缩短或去符号，需保证仍可读、不撞名（靠 `[bvid_pN]` 区分）。
