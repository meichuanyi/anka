# AnkiWeb 一次性迁移导入（设计）

> 决策（2026-09-18）：做"从 AnkiWeb 一次性全量拉取"的迁移功能。
> 定位：独立模块 `anka-ankiweb`，可整体移除（kill switch）。
> **不做**持续双向同步（协议最难且风险最高的部分）。

## 原则

- 密码不落盘：用户在客户端界面输入，设备直连 AnkiWeb，用完即弃。
- 行为等同 Anki 官方客户端的全量下载（"从 AnkiWeb 下载"按钮）。
- 协议参考 Anki 开源客户端（AGPL）实现，本项目同为 AGPL，许可兼容。
- 若官方明确反对：删除本模块即完全移除此渠道。

## 协议要点（源自 rslib/src/sync，已在 anki-fork 源码中核实）

- 基址：`https://sync.ankiweb.net/`，方法名即路径（如 `/sync/hostKey`）。
- 请求头 `anki-sync`：JSON 字符串
  `{"v": <协议版本int>, "k": "<hkey>", "c": "<客户端版本>", "s": "<session key>"}`
  （登录时 k/s 为空）。
- 请求体：JSON 序列化后 **zstd 压缩**（hostKey 为 `{"u": 用户名, "p": 密码}`）。
- hostKey 响应：`{"key": "<hkey>"}`；**凭证错误 → HTTP 403**（可作联调里程碑）。
- 全量下载：携带 hkey 请求 download → 响应体即 **collection.anki2 文件字节**
  → 直接喂 `anka-apkg` 现有 V11 导入管线（无需新解析代码）。
- 媒体走独立 msync 端点（meta 返回）——**列为二期**，一期先拉卡组数据。

## 移植清单（自 rslib/src/sync，剥离 anki 框架依赖）

| 源文件 | 移植内容 | 需剥离 |
|---|---|---|
| `login.rs` | sync_login（hostKey） | prelude/anki_proto → 手写 struct |
| `http_client/mod.rs` | 请求组装（头+压缩体+重试） | 进度回调简化 |
| `request/`、`version.rs` | SyncHeader 编码、版本协商 | axum_extra Header → 手写 HTTP 头 |
| `collection/download.rs` | full_download 流程 | anki_io → std |

## 集成

- `crates/anka-ankiweb`（lib）：`login() -> hkey`、`full_download(hkey) -> Vec<u8>`。
- 导入：下载字节写临时 `collection.anki2` → `anka-apkg` 现有导入 → Anka 收藏。
- UI：⚙ 设置页新增「从 AnkiWeb 导入」表单（地址固定 AnkiWeb，账号/密码 + 进度）。
- 前端三处同源（桌面 IPC / serve / server）均可用；导入逻辑放客户端侧执行。

## 测试计划

1. 联调里程碑：错误凭证 → 期望 AnkiWeb 返回 403（证明线上格式正确）。
2. 真实账号全量拉取（用户侧实测，密码不经过任何第三方）。
3. 拉取结果跑导入 → 抽查卡组/卡数/媒体映射。

## 二期：双向同步（已立项）

为"没有服务器"的用户提供完整多端同步：Anka 作为 Anki 生态的同步客户端，
AnkiWeb 就是他们现成的免费中枢，可继续与 Anki/AnkiDroid 混用。

**方案：内嵌官方引擎（不自己实现增量协议）**

- 依赖官方 `anki` crate（AGPL，兼容本项目许可），即 Anki Desktop / AnkiDroid
  实际使用的同一套同步引擎——协议行为逐字节一致，上游升级替我们维护兼容性。
- 架构：App 数据目录内维护一个隐藏的 Anki 格式收藏 `sync-agent.anki2` 作为
  同步代理；Anka 自己的 `collection.akdb` 仍是唯一"真相"。
- 流程：Anka 变更 → 按 `anki_id_map` 映射写入 agent 收藏 → 官方引擎与 AnkiWeb
  双向同步 → 拉回的变更映射回 Anka。
- FSRS：现代 Anki 卡片本身就携带 FSRS 记忆状态（v3 调度器），可直接映射。
- 媒体同步由官方引擎处理（msync 协议）。

**里程碑**

| 里程碑 | 内容 | 量级 |
|--------|------|------|
| M-A | vendor `anki` crate，编译打通 | 数天 |
| M-B | agent 收藏与 AnkiWeb 全量往返 | 1 周 |
| M-C | 映射层：notes/cards/decks/revlog 双向按 GUID 同步 | 2-3 周 |
| M-D | 媒体同步 + 冲突提示 | 1-2 周 |
| M-E | UI（同步按钮/进度/账号管理） | 1 周 |

**一期（一次性导入）在桌面/手机 App 上已是客户端直连**（Tauri 命令
`ankiweb_import`，不经过任何服务器）；浏览器模式因无法运行同步协议，
只能经由用户自己的库服务器执行（页面有明确提示）。

## 明确不做

- 密码/凭证的任何持久化
- 将 AnkiWeb 作为 Anka 官方托管后端（我们仍不提供服务器）
