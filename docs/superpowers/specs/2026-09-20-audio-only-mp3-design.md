# 仅下载音频 (MP3) 设计

日期：2026-09-20  
状态：已口头批准，待书面确认后实现

## 目标

下载页增加「仅下载音频」选项；勾选后下载最好音质的 MP3，并出现在媒体库中。

## 行为

- UI：下载页清晰度旁增加勾选框「仅下载音频 (MP3)」
- 勾选后：清晰度选择器 `disabled`（值保留但不参与下载）
- 未勾选：行为与现有视频下载完全一致
- 音频模式固定最好音质（yt-dlp `--audio-quality 0`），不提供档位

## API / 后端

- `StartDownloadArgs` 增加 `audio_only: bool`（前端 `audioOnly`）
- `api.startDownload(url, category, quality, audioOnly)`
- `audio_only == true` 时 yt-dlp 参数：
  - `-x --audio-format mp3 --audio-quality 0`
  - 不使用视频 `-f` / `--merge-output-format mp4`
- 分类、代理、进度事件、单任务锁与现有逻辑相同
- 启动日志标明：音频模式 · 最好音质 · mp3
- 转码失败时错误信息提示检查本机是否安装 ffmpeg

## 媒体库

- `library::VIDEO_EXTS` 增加 `mp3`，以便列表/打开/移动/删除可用

## 非目标

- 不改设置页默认清晰度
- 不新增音质档位 UI
- 不单独拆「下载音频」按钮或视频/音频模式切换

## 依赖

- 本机需有 ffmpeg（yt-dlp 提取/转码 MP3 常用）；缺失时由错误提示引导安装
