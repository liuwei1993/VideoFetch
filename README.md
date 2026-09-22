# VideoFetch

Tauri 桌面客户端：下载 YouTube / Bilibili / MissAV 视频，并用 `~/videofetch/<分类>/` 文件夹管理分类。

## 依赖

- Node.js 18+
- Rust（stable）
- [ffmpeg](https://ffmpeg.org/) 与 [yt-dlp](https://github.com/yt-dlp/yt-dlp)：开发时可用系统安装，或运行 `bash scripts/fetch-linux-sidecars.sh` 下载内置副本。**AppImage 已内置** yt-dlp、ffmpeg、ffprobe（x86_64 / aarch64 glibc；Alpine 等 musl 系统不支持）。
- [uv](https://github.com/astral-sh/uv) 仅作开发机回退（没有内置二进制时），不会打进 AppImage。
- 访问 YouTube / MissAV 时需要本机 HTTP 代理（默认 `http://127.0.0.1:57890`）
- YouTube 抽取建议安装 Node（`--js-runtimes node`）

可选环境变量：

- `VIDEOFETCH_YTDLP`：自定义 yt-dlp 可执行文件路径

## 开发

```bash
npm install
npm run tauri dev
```

## 使用

1. **下载**：粘贴链接，选择/新建分类，选清晰度（默认 720p），开始下载。支持 YouTube 频道/播放列表（如 `youtube.com/@handle/videos`、`/playlist?list=...`）以及 Bilibili 合集（`space.bilibili.com/.../lists/...?type=season`）：展开后批量下载，下载页显示每集状态。MissAV（`missav.ws` / `missav.com` / `missav.ai`）目前仅支持单视频链接，例如 `https://missav.ws/cn/<番号>`。  
   若上次下载未完成（崩溃或强制退出），启动时会提示「继续 / 丢弃」。继续将跳过已完成项并用 yt-dlp 断点续传；丢弃会清除队列与临时文件。主动点「停止」不会提示恢复。  
2. **库**：左侧分类，右侧视频；可打开、移动、删除。  
3. **设置**：库根目录、默认清晰度、YouTube 代理、是否让 Bilibili 走代理、同时下载数（默认 5）。

默认库目录：`~/videofetch`。设置保存在应用 config 目录的 `settings.json`。

## 代理策略

| 站点 | 默认 |
|---|---|
| YouTube | 使用设置中的代理 |
| MissAV | 与 YouTube 相同的代理 |
| Bilibili | 直连 |

## 设计文档

见 `docs/superpowers/specs/2026-09-20-tauri-video-downloader-design.md`。
