# Bilibili 视频下载工具（Tauri 2 + Rust）

一个仅面向你有权保存的公开视频的桌面程序。输入 BV 号、AV 号、`bilibili.com/video/...` 页面链接或 `b23.tv` 短链接，即可解析标题、封面和可用清晰度，然后下载。程序通过 yt-dlp 下载，并由 yt-dlp 调用 FFmpeg 合并分离的音视频流。画面截取页可打开本地视频，拖动进度条定位并将当前画面保存为 PNG。下载完成的视频也会自动载入该页面。不提供登录、DRM 绕过或访问限制绕过功能。

## 项目结构

```text
bilibili-tauri/
├─ index.html                 界面
├─ src/main.js                前端交互、进度事件
├─ src/style.css              样式
├─ package.json               前端和 Tauri CLI 依赖
├─ vite.config.js             本地开发服务端口
├─ scripts/generate-icon.mjs  生成 Windows 图标
└─ src-tauri/
   ├─ Cargo.toml              Rust 依赖
   ├─ tauri.conf.json         Tauri 和 NSIS 配置
   ├─ capabilities/default.json
   ├─ src/main.rs
   └─ src/lib.rs              链接校验、解析、下载、进度事件
```

## Windows 环境准备

1. 安装 [Rust](https://rustup.rs/)、[Node.js LTS](https://nodejs.org/) 和 [Tauri Windows 前置依赖](https://v2.tauri.app/start/prerequisites/)（Microsoft C++ Build Tools、WebView2）。
2. 安装 [yt-dlp](https://github.com/yt-dlp/yt-dlp/releases) 与支持 PNG 图片输出的[完整版 FFmpeg](https://ffmpeg.org/download.html)。将 `yt-dlp.exe`、`ffmpeg.exe` 所在目录加入系统 `PATH`。在新开的终端确认 `yt-dlp --version` 和 `ffmpeg -version` 均可运行。精简版 FFmpeg 可能只能合并视频，无法保存截图。
3. 在本项目目录运行：

```powershell
npm install
node scripts/generate-icon.mjs
npm run tauri dev
```

工具会直接调用 PATH 中的 yt-dlp 和 FFmpeg。安装包不会内置这两个程序；在目标电脑上也需要安装它们。

## 构建 Windows `.exe` 安装包

```powershell
npm run tauri build
```

构建完成后，NSIS 安装包位于 `src-tauri/target/release/bundle/nsis/`，文件名以 `_x64-setup.exe` 等结尾。生成的是安装程序；安装后应用程序本身也是 `.exe`。参见 [Tauri Windows 打包文档](https://v2.tauri.app/distribute/windows-installer/)。

## 使用限制

- 默认只处理单个视频；多 P 页面可以用 `?p=数字` 指定页面。
- 清晰度列表来自 yt-dlp 对公开视频的解析结果；网站实际可提供的清晰度和下载权限可能随视频而不同。
- 下载期间保持程序窗口打开。当前版本一次只执行一个下载任务，没有暂停或续传界面。
- 网络请求及格式识别由 yt-dlp 负责。封面由 yt-dlp 下载后在界面中显示，避免站点阻止 WebView 直接读取图片。出现站点变更时，先更新 yt-dlp。
- 点击右上角齿轮设置保存目录。首次启动会创建程序同目录的 `download` 文件夹；如果该目录不可写，则使用应用本地数据目录中的 `download` 文件夹。下载的视频和截图共用此目录。
- 若 PATH 中的 FFmpeg 不支持 PNG 截图，可在齿轮设置中选择另一份完整版 `ffmpeg.exe`；所选路径会保存在本机界面设置中。
