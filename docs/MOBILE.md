# Anka Mobile (Tauri 2 — Android / iOS)

Anka 内核是纯 Rust 库，移动端与桌面端**共用同一套前端和 IPC 命令**，没有第二套业务逻辑。

## 双模式架构

```text
┌─ 手机 App ──────────────────────────────────────────┐
│  客户端模式（推荐，默认产品主线）                       │
│    ⚙ 填入自托管 anka-server 地址 + Token              │
│    → 全部 API 走 HTTP（Bearer），数据在 NAS/VPS       │
│    → 与桌面、Agent（MCP）实时同库                      │
│                                                      │
│  本地模式（离线备用）                                  │
│    → Tauri IPC 直连进程内 anka-core                    │
│    → 收藏位于应用沙箱: {app_data_dir}/collection.akdb  │
│    → 跨设备经 anka sync push|pull                     │
└──────────────────────────────────────────────────────┘
```

- 传输选择在 `apps/desktop/src/main.ts`：`native() = isTauri() && 未配置远程`。
- 远程模式设置存 `localStorage`（`anka.server` / `anka.token`），首页 `⚙` 进入。
- Android 默认禁止明文 HTTP，工程已开启 `usesCleartextTraffic`（局域网自托管是 `http://`）。

## Android 构建

要求：JDK 17、Android SDK（platforms;android-34、build-tools;34.0.0）、NDK 27、Rust targets：

```bash
rustup target add aarch64-linux-android armv7-linux-androideabi \
                  i686-linux-android x86_64-linux-android
export JAVA_HOME=/usr/lib/jvm/java-17-openjdk-amd64
export ANDROID_HOME=/opt/android-sdk
export NDK_HOME=$ANDROID_HOME/ndk/27.0.12077973
```

```bash
cd apps/desktop
npm install
npx tauri android init          # 首次：生成 gen/android
npx tauri android build --apk --target aarch64   # 64 位 APK
# 产物: src-tauri/gen/android/app/build/outputs/apk/universal/arm64/release/*.apk
```

调试构建（自动 debug 签名，可直接安装）：

```bash
npx tauri android build --apk --debug --target aarch64
```

### Release 签名

```bash
keytool -genkeypair -v -keystore anka.keystore -alias anka \
  -keyalg RSA -keysize 2048 -validity 10000
# 在 gen/android/keystore.properties 或按 Tauri 文档配置 signingConfig 后:
npx tauri android build --apk --target aarch64
```

## iOS 构建（需要 macOS + Xcode）

Linux 无法编译 iOS。在 Mac 上：

```bash
# 前置: xcode-select --install && cargo install tauri-cli  (或使用 npm CLI)
rustup target add aarch64-apple-ios aarch64-apple-ios-sim
cd apps/desktop
npm install
npx tauri ios init
npx tauri ios dev          # 模拟器/真机调试
npx tauri ios build        # 产出 Xcode 工程，归档/签名走 Xcode
```

AppIcon 已由 `npx tauri icon app-icon.png` 生成（`src-tauri/icons/ios/`），init 会自动带入。

## 已知差异 / 注意

| 事项 | 说明 |
|------|------|
| 明文 HTTP | 仅建议局域网/内网使用 `http://`；公网务必上 HTTPS（反向代理即可） |
| 本地模式媒体 | Tauri 模式音频经 asset protocol（`convertFileSrc`）；远程模式走 `/media/*` |
| 每次冷启动 | 本地模式首次进入会自动创建空收藏；要复用数据请走客户端模式或 sync |
| 图标 | 源文件 `app-icon.png`（图腾 1024px），改图后重跑 `npx tauri icon app-icon.png` |
