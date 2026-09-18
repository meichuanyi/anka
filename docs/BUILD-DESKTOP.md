# 桌面 App 构建指南

## Windows（在你的 Windows 电脑上，约 10 分钟）

前置（5 月构建过的话环境都在）：Rust (MSVC)、Node 18+、VS Build Tools、WebView2（Win10/11 自带）。

```powershell
git clone https://github.com/meichuanyi/anka.git
cd anka\apps\desktop
npm install
npm run tauri build
```

产物位置：
- 安装包 `src-tauri\target\release\bundle\nsis\*-setup.exe`
- 绿色版 `src-tauri\target\release\anka-desktop.exe`

开发调试（热重载）：
```powershell
npm run tauri dev
```

构建完成后把 AnkiWeb 账号填进 ⚙ 设置，即可双向同步。

## Linux（本机构建机或任意 Linux 桌面）

```bash
sudo apt install libgtk-3-dev libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev
cargo build --release -p anka-desktop
# 二进制: target/release/anka-desktop
```

## Android

见 docs/MOBILE.md（在配置好 SDK/NDK 的机器上 `npx tauri android build --apk`）。

## iOS

需要 macOS + Xcode，见 docs/MOBILE.md。
