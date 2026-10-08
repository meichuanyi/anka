# Anka 背诵（VSCode 插件）

在 VSCode **状态栏（最下面一行）**隐蔽复习 Anka 卡片——屏幕上只有一个单词，适合在工位上悄悄背。

数据直连你的自建 anka-server，与手机 App / 网页版共用同一收藏和 FSRS 进度。

## 安装

```bash
# 本仓库已产出 vsix（apps/vscode/anka-0.1.0.vsix）
code --install-extension anka-0.1.0.vsix
# 远程开发（SSH/WSL）时，在远程窗口的扩展面板里
# 「从 VSIX 安装…」，或在集成终端里执行上面的命令
```

## 配置

打开设置搜 `anka`：

| 配置 | 说明 |
|------|------|
| `anka.server` | anka-server 地址，如 `http://192.168.6.100:8788` |
| `anka.token` | 访问令牌（服务器启动日志 `#t=` 后的一串） |
| `anka.deck` | 只复习指定牌组（空 = 全部到期） |
| `anka.revealSeconds` | 显示答案几秒后自动收回（默认 8） |
| `anka.autoRefreshMinutes` | 自动拉取间隔（默认 15 分钟） |
| `anka.pullSize` | 每批拉取张数（默认 20） |
| `anka.showCount` | 状态栏是否显示进度 N/M |

## 使用（全键盘，最隐蔽）

| 按键 | 作用 |
|------|------|
| 状态栏 | 平时只显示 `📖 单词 N/M`，悬停不剧透答案 |
| `Ctrl+Alt+Space` | 状态栏/悬停显示答案，几秒后自动收回 |
| `Ctrl+Alt+1 / 2 / 3 / 4` | Again / Hard / Good / Easy |
| `Ctrl+Alt+→` | 跳过这张 |
| 点状态栏 | 弹出评分面板（含答案） |

评分后自动换下一张；本批完成自动重新拉取到期卡片。答题进度实时写入服务器，手机/网页打开就是最新进度。

## 开发

```bash
npm install
npm run compile
npx vsce package   # 产出 anka-0.1.0.vsix
```
