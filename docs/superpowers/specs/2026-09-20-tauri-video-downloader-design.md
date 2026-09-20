# Tauri 视频下载器设计

日期：2026-09-20  
状态：已批准（对话确认）

## 目标

桌面客户端：下载 YouTube / Bilibili 视频，并用磁盘文件夹做分类管理。

## 技术栈

| 层 | 选择 | 说明 |
|---|---|---|
| 壳 | Tauri 2 | 轻量桌面壳 |
| 前端 | React 19 + TypeScript + Vite | 用户选择 |
| 下载引擎 | yt-dlp 侧车二进制 | 统一支持 YouTube / Bilibili |
| 合并转码 | ffmpeg（系统或捆绑） | DASH 音视频合成 mp4 |
| 配置 | 应用 config 目录下的 `settings.json` | 见下文 |
| 视频库根目录 | 默认 `~/web-videos` | 设置可改 |

不采用：纯 Rust 站点解析、Electron。

## 目录与分类

分类 = 库根目录下的**一级文件夹**（第一版不支持嵌套）。

```
~/web-videos/
  未分类/
  脱口秀/
  教程/
  …
```

| 操作 | 行为 |
|---|---|
| 新建分类 | 创建同名文件夹 |
| 重命名分类 | 重命名文件夹 |
| 删除分类 | 删除文件夹；非空时需确认 |
| 移动视频 | 文件系统移动到目标分类目录 |
| 下载落盘 | `~/web-videos/<分类>/<标题> [id].mp4` |

回退分类名：`未分类`（尚未选过分类时使用）。

「上次使用的分类」写入 settings；下载表单默认选中它，仍可改选或新建。

## 主界面

三个主视图：

1. **下载**  
   - 输入 URL  
   - 分类下拉（可新建）  
   - 清晰度（跟随默认，可临时改）  
   - 开始按钮、进度条、简要日志  
   - 按 URL 自动识别站点（youtube.com / youtu.be / bilibili.com / b23.tv 等）

2. **库**  
   - 左侧：分类列表  
   - 右侧：该分类下视频（标题、时长、大小等，能从文件/旁路元数据拿到的信息）  
   - 操作：移动分类、用系统默认播放器打开、删除文件

3. **设置**  
   - 库根目录（默认 `~/web-videos`）  
   - 默认清晰度（默认 720p）  
   - YouTube 代理（默认 `http://127.0.0.1:57890`）  
   - Bilibili 是否走代理（默认关闭 = 直连）  
   - Cookie 文件路径（第一版仅保留设置字段与持久化，下载流程暂不接入）

## 代理策略

| 站点 | 默认行为 |
|---|---|
| YouTube | 使用设置中的代理 |
| Bilibili | 直连 |

站点由 URL 判定；调用 yt-dlp 时仅对 YouTube（或用户勾选「Bilibili 也走代理」时）传入 `--proxy`。

## 下载行为

- 默认格式选择器（720p）：`bv*[height<=720]+ba/b[height<=720]`，`--merge-output-format mp4`  
- 清晰度随设置改为对应 `height`（如 1080 → `<=1080`）；「最高」用合适的 best 选择器  
- 需要 Node（或其它 JS runtime）时：侧车调用加上 `--js-runtimes node`（环境有 Node 时）  
- 进度：Rust 解析 yt-dlp 输出，经 Tauri event 推到前端  
- 落盘：先写临时文件，成功后再移到最终路径，避免半成品占用正式文件名  
- 下载成功后更新 `last_category`

## 配置结构（示意）

```json
{
  "library_root": "~/web-videos",
  "default_quality": "720",
  "last_category": "脱口秀",
  "youtube_proxy": "http://127.0.0.1:57890",
  "bilibili_use_proxy": false,
  "cookie_file": null
}
```

路径在运行时展开 `~` 为用户家目录。

## 后端职责（Rust / Tauri commands）

- `get_settings` / `save_settings`
- `list_categories` / `create_category` / `rename_category` / `delete_category`
- `list_videos(category)` / `move_video` / `delete_video` / `open_video`
- `start_download({ url, category, quality })` + 进度/完成/失败 events
- `ensure_library_root`（启动时创建根目录与「未分类」）

## 第一版明确不做

- 播放列表 / 批量队列调度 UI（可后续加）
- 应用内视频播放器
- 多标签、嵌套分类
- 云同步
- 复杂的自动更新以外的调度系统

## 验证标准

1. 配置 YouTube 代理后，能下载已知可用的 YouTube URL 到所选分类目录，得到可播放 mp4。  
2. Bilibili 在默认直连下能下载到所选分类。  
3. 新建分类后出现在库侧栏；视频可移动到另一分类，磁盘路径同步变化。  
4. 重启应用后「上次分类」与设置项仍保留。  
5. 默认清晰度为 720p，设置改为 1080p 后新任务按新高度选择格式。

## 风险与依赖

- 本机访问 YouTube 依赖用户代理；DNS 劫持时无代理会失败（已在本机验证过 `127.0.0.1:57890`）。  
- 系统 yt-dlp 过旧不可靠；应用应捆绑或固定较新版本侧车。  
- ffmpeg 必须可用，否则无法合并分离流。  
- YouTube 抽取可能需要 JS runtime（Node）；文档中说明依赖。
