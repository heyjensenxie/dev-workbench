# Dev Workbench

[中文](README.md) | [English](README.en.md)

<p align="center">
  <img src="apps/desktop/src/assets/logo.png" alt="Dev Workbench logo" width="120" height="120">
</p>

<p align="center">
  <a href="https://github.com/heyjensenxie/dev-workbench/actions/workflows/ci.yml"><img src="https://github.com/heyjensenxie/dev-workbench/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-blue.svg" alt="License: Apache-2.0"></a>
  <img src="https://img.shields.io/badge/status-alpha-orange.svg" alt="Status: alpha">
</p>

> 面向现代软件开发、本地优先且可扩展的开发者工作台。

Dev Workbench 是围绕项目构建的桌面环境，用于理解和运行本地软件项目。它不堆砌互不相关的工具，而是把项目、服务、进程、端口、日志与命令上下文收敛在稳定的应用边界之后，供 UI、插件、快捷键以及未来的 AI 集成共同使用。

当前版本：**v0.1.0-alpha.2**（预发布；安装包未签名）

## 设计取向

三个决定塑造了整个项目，也解释了它为什么不是又一个工具合集：

1. **本地优先，且默认不联网。** 项目记录、服务定义、设置、API 请求模板与数据库连接都存在本机 SQLite；只有你显式点「发送」的 HTTP 请求会离开这台机器。
2. **凭据与普通数据物理隔离。** 数据库密码进入操作系统凭据库，密码库则是一个完全独立的加密域（独立的 `vault.db`、独立的密钥层级、不注册进共享服务容器），任何插件与未来的 AI 面都拿不到它。
3. **无法兑现的能力就不写进文档。** `crates/{docker,git,pty}` 与 `plugins/*` 目前只是**所有权边界**，没有任何实现；README 与 `docs/` 不把它们描述成已交付功能。

## 当前能力

### 项目与服务

- Tauri 2 + Vue 3 桌面外壳，支持明暗主题与中英文界面；首次启动的语言跟随操作系统
- 项目数据持久化到本地 SQLite 数据库，按版本一次性应用并记录迁移
- Projects 页面支持添加、打开、刷新、在文件管理器中显示，以及安全移除工作台记录（不会删除磁盘文件）
- 项目扫描器：识别语言、包管理器、框架、清单/锁文件、容器与 CI 配置、Agent 指令，并解析 `package.json`、`Cargo.toml`、compose 文件与 `Makefile`
- 基于扫描结果给出可一键导入的服务建议（脚本、compose 服务、make 目标），并读取当前 Git 分支
- 服务管理闭环：新增、编辑、删除服务，编辑参数、环境变量、工作目录、端口与依赖关系
- 服务启动、停止、重启，按依赖顺序的全部启动与全部停止；依赖启动失败时自动跳过下游服务
- 托管本地子进程，终止时连同子进程树一起清理，避免残留进程占用端口
- 运行中的服务状态与 PID 实时同步，服务自行退出时界面会收到事件

### 进程、端口与日志

- 进程页：可筛选的进程列表（内存、父进程、启动时间），支持按内存或进程树排序，并可直接结束进程树
- 端口页：监听端口与占用进程，一键释放
- 服务配置端口与监听端口自动关联，并在启动前提示端口占用者
- 按服务过滤、搜索、暂停/恢复自动滚动、清空并高亮 ERROR/WARN 的运行时日志面板
- 跨平台的进程清单（含父进程与内存）与监听端口探测、进程名解析

### API Workbench（HTTP 客户端）

- 请求编辑器：查询参数、请求头、JSON 请求体，方法、超时与 `Ctrl / ⌘ + Enter` 发送
- 保存的请求模板按模块分组，并保留最近请求历史；历史 URL 会先脱敏（去掉 userinfo、替换疑似 token 的查询值）再展示
- 环境变量只存在于会话内存，发送前才代入 `{{VAR}}` 模板，因此保存的记录里不会留下密钥
- 响应查看：状态、耗时、大小，格式化/原始/响应头三种视图

### Database Workbench（数据库客户端）

- MySQL 与 SQLite 连接配置；数据库密码写入操作系统凭据库，连接记录里只保留一个 `secretRef` 标记
- 库表结构树，多标签 SQL 编辑器
- 结果、消息、执行计划、表结构、DDL 与查询历史分栏
- 执行前对破坏性语句（`DROP`、`TRUNCATE`、`ALTER TABLE`、无 `WHERE` 的 `UPDATE`/`DELETE`）做本地静态分析并二次确认
- 纯函数实现的 SQL 分析（尊重引号内分号的语句切分、只读判定、结果行数上限）在 `packages/core` 中单独测试，不需要真实数据库

### Password Vault（本地密码库）

- 独立的加密域：主密码经 Argon2id 派生 KEK，KEK 只用于包裹随机生成的 256 位库主密钥（VMK），每条记录再用 XChaCha20-Poly1305 加密后写入独立的 `vault.db`
- 表结构里**没有任何** `title` / `username` / `password` / `url` / `notes` 列：整个记录体序列化后整体加密，只有不透明的 id、nonce、密文与时间戳
- 条目增删改查、标签、收藏、敏感自定义字段，以及在解密摘要上的内存内搜索
- CSPRNG 口令生成器；复制口令后按时自动清除，且仅当剪贴板内容仍是自己写入的值时才清除
- 加密 `.vaultbackup` 导出与还原（还原前会先校验文件头、包裹密钥与每条记录，并写一份带时间戳的安全副本）
- 主密码修改、加密强度重新基准化、退出/空闲/锁屏/休眠/隐藏窗口/`Ctrl + Shift + L` 多重自动锁定，以及递进式失败锁定
- 自动化威胁模型测试套件 `crates/vault/tests/security.rs`，断言明文（主密码、用户名、密码、备注、标题、URL、标签、自定义字段）永不落入库文件、WAL 或副作用文件

### 应用外壳

- Utilities 页面：本地 JSON 格式化/压缩、JWT 解码、时间戳转换、UUID 生成、Base64 与 URL 编解码、SHA-1/SHA-256 Hash、正则测试，以及 SQL 格式化/压缩/IN 构造/参数填充/DDL 预览与 MyBatis 日志还原
- 设置页：系统/深色/浅色主题、语言、启动时恢复上次项目、日志保留行数与「结束进程前确认」，持久化到本地数据库
- 共享上下文存储与类型化命令注册表，`Ctrl+K` 模糊匹配命令面板
- 生产构建启用严格 CSP（脚本与样式仅限打包资源，连接仅允许 Tauri IPC 与自身，禁止 object/iframe/表单提交）

## 平台支持与已知限制

这是一个 alpha 版本，下面的限制是**已知且有意公开**的，请按实际能力评估是否适合你：

| 能力 | Windows | Linux | macOS |
|---|---|---|---|
| 项目 / 服务 / 进程 / 端口 / 日志 | ✅ | ✅ | ✅ |
| API Workbench | ✅ | ✅ | ✅ |
| Database Workbench（查询与结构树） | ✅ | ✅ | ✅ |
| 数据库密码写入操作系统凭据库 | ✅ Windows 凭据管理器 | ⚠️ 未实现 | ⚠️ 未实现 |
| Password Vault | ✅ | ✅ | ✅ |
| 密码库密钥页锁定（防交换文件） | ✅ `VirtualLock` | ✅ `mlock` | ✅ `mlock` |
| 剪贴板自动清除 | ✅ | ⚠️ 见下 | ⚠️ 见下 |

- **`crates/secrets` 目前只有 Windows 实现。** 在 Linux 与 macOS 上，保存数据库密码会明确返回「OS secret storage is not implemented on this platform」，而不是静默丢弃；查询本身不受影响。
- **剪贴板自动清除依赖原生实现**，在没有实现的平台上命令会直接失败，界面会说明「自动清除不可用」，而不是假装成功。手动复制仍然可用。
- **密码库尚未经过独立安全审计。** 它已实现并有自动化威胁模型测试，但按照 [docs/vault-security.md](docs/vault-security.md) §11 的定义，进入公开 Beta 前仍需一次独立安全评审。在评审完成前，请只在**自己的机器上**使用它，不要用于团队共享或生产凭据。
- **没有密码找回。** 没有服务器、没有账户、没有恢复密钥、没有后门。忘记主密码即等于丢失密码库。
- **安装包未签名。** 系统可能要求你显式确认「仍要打开」。Windows 安装包、macOS `.dmg`、Linux `.deb`/`.AppImage` 均由 `release.yml` 构建为草稿 Release，需人工确认后才发布。
- **数据位置**：应用数据（含 `workbench.sqlite3` 与独立的 `vault.db`）保存在各平台 Tauri 应用数据目录下。

## 本版本不包含

Redis 工具、完整的 Docker 或 Git UI、MCP Inspector、AI 功能、插件市场、Kubernetes 或 SSH 支持。对应目录（`crates/{docker,git,pty}`、`plugins/*`）只是预留的所有权边界，没有任何实现，也不会被描述成已交付能力。

## 技术栈

| 层 | 选型 | 在本项目里承担什么 |
|---|---|---|
| 桌面外壳 | Tauri 2 + Rust（edition 2024）、Tokio、Serde | 窗口、IPC 与全部原生能力。复用系统 WebView，不内嵌 Chromium，因此安装包比 Electron 方案小得多 |
| 前端 | Vue 3 + TypeScript（strict）+ Vite | 视图、路由与交互。`vue-tsc` 在 `build` 和 CI 里都会跑，类型检查不是可选项 |
| 状态与路由 | Pinia + Vue Router（hash 模式） | 四个 store 分别持有工作台、系统、设置与密码库的视图状态 |
| UI | Tailwind CSS 4 + Reka UI + Lucide | 设计令牌与无样式可访问组件；样式集中在 `apps/desktop`，不向其他包扩散 |
| 持久化 | SQLite + SQLx（异步） | 项目、服务、设置、API 请求模板、数据库连接配置；版本化迁移在事务内一次性应用并记录 |
| 密码学 | RustCrypto `argon2` + `chacha20poly1305` | 密码库的 KDF 与 AEAD。没有自研构造、没有 XOR 方案、没有未认证的 CBC |
| 原生适配 | `crates/{process,system,network}` | 进程树管理、进程清单、端口探测。操作系统差异只存在于这三个 crate 内部 |
| 工作区 | pnpm + Cargo workspace | 13 个前端包与 9 个 Rust crate，前端与后端各一份锁文件 |

## 它是怎么工作的

**依赖方向是单向的。** `Vue UI → Application Service → Command System → Native Bridge → Tauri Command → Rust`。整个前端只有 `apps/desktop/src/services/nativeBridge.ts` 一个文件调用 `invoke`，其余代码一律走应用服务或命令注册表。这条约束换来的是可测试性：上层可以在 Node 里用一个假 bridge 加上真实的 `createWorkbench` 跑完，`apps/desktop/src/stores/*.test.ts` 就是这么做的。

**原生面是有界的。** 全部原生能力就是注册在 `apps/desktop/src-tauri/src/lib.rs` 的 56 个 Tauri 命令，其中 23 个属于密码库。密码库另有两条硬约束：不注册进共享服务容器（所以插件拿不到），并且只暴露 `vault.open` 与 `vault.lock` 两个命令。任何新的原生能力都必须显式走进这份清单，不会「顺手」多出来。

**进程归属是明确的。** 应用只管理自己启动的进程。停止服务时会扫掉整棵进程树——Windows 用 `taskkill /T /F`，因为系统记录的父子链是唯一能可靠触达孙进程的方式；Unix 则把服务放进独立进程组，先 `kill -TERM -<pgid>`，宽限期结束后升级为 `-KILL`。日志读取器在停止时主动 `abort`，否则被存活的后代进程继承的管道句柄会把读取器一直吊在那里。

**数据分三处落盘，各有各的理由。**

| 位置 | 存什么 | 为什么分开 |
|---|---|---|
| `workbench.sqlite3` | 项目、服务、设置、API 请求模板、数据库连接配置 | 普通业务数据，多个功能共享同一读写路径 |
| `vault.db` | 密码库 | 表里没有标题/用户名/密码/URL/备注任何一列，写进去的就已经是密文，所以 WAL 和临时文件也只可能是密文 |
| 操作系统凭据库 | 数据库连接密码 | 业务库里只留一个 `secretRef` 标记，密码不落在磁盘上 |

**跨平台差异被关在适配层里。** `crates/system`、`crates/network`、`crates/process` 各自提供 Windows 与 Unix 实现，其余代码不写平台判断。这条规则也让 `crates/secrets` 的现状一眼可见：它目前只有 Windows 实现，其他平台会明确报错而不是静默降级。

**安全是默认收窄，而不是逐项加固。** 生产构建启用严格 CSP（脚本与样式仅限打包资源，禁止 object/iframe/表单提交）；API 请求的 URL 只接受 `http`/`https`；数据库的破坏性语句在执行前先做本地静态分析并二次确认。

## 快速开始

### 运行已构建的安装包

从 [Releases](https://github.com/heyjensenxie/dev-workbench/releases) 下载对应平台的安装包。安装包未签名，首次打开时系统可能要求确认。

### 从源码运行

前置条件：Node.js 22+、pnpm 11+、stable Rust 工具链，以及对应操作系统的 [Tauri 系统依赖](https://v2.tauri.app/start/prerequisites/)。Node 与 Rust 版本分别由 [`.nvmrc`](.nvmrc) 和 [`rust-toolchain.toml`](rust-toolchain.toml) 固定。

```bash
pnpm install
pnpm dev          # Vite 开发服务器 + Tauri 桌面窗口
pnpm dev:web      # 仅浏览器端界面，不含原生外壳
```

生成当前平台的安装包（MSI / NSIS，用于分发）：

```bash
pnpm --filter @dev-workbench/desktop tauri build
```

生成免安装的 Windows 便携版单文件应用（自用推荐）：

```bash
pnpm build:portable
```

产物为 `release/Dev Workbench.exe`，双击即可运行，无需安装或管理员权限；唯一的外部依赖是 Windows 10/11 自带的 Edge WebView2 运行时。文件名不含版本号，重新打包后任务栏固定项依旧有效。详见 [docs/development.md](docs/development.md#packaging)。

## 质量检查

```bash
pnpm typecheck
pnpm test
pnpm build
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo check --workspace
```

以上全部命令也就是 [`.github/workflows/ci.yml`](.github/workflows/ci.yml) 在每次推送 `main` 与每个 PR 上执行的内容。前端 223 个 Vitest 用例（`packages/{shared,command,context,core}`、`apps/desktop`）与 Rust 侧 196 个用例（其中 `crates/vault` 占 144 个）覆盖纯逻辑、存储、解析器与安全边界。

## 发布

推送 `v*` 标签时，[`.github/workflows/release.yml`](.github/workflows/release.yml) 会为 Linux、Windows 与 macOS（含 Apple Silicon 与 Intel 两个目标）构建安装包，并创建一个**草稿** Release 等待人工确认。

## 仓库结构

- `apps/desktop`：桌面端组合、路由、视图，以及唯一的 Tauri 桥接实现
- `packages/shared`：跨边界模型、校验规则与结构化错误
- `packages/command`：命令契约、注册表、执行与模糊匹配
- `packages/context`：可观察的工作台/项目上下文
- `packages/core`：应用服务（项目、服务目录、服务管理、系统、设置、API、数据库）与核心命令注册
- `packages/ui`：共享设计令牌与未来的可复用基础组件
- `packages/plugin-api`、`packages/sdk`：刻意保持精简的扩展契约
- `crates/process`：托管子进程运行时、进程树终止与日志流
- `crates/system`、`crates/network`：面向操作系统的进程与端口适配器
- `crates/secrets`：操作系统凭据库边界（当前仅 Windows 实现）
- `crates/vault`：密码库的密码学、KDF、页锁定内存、会话与存储实现
- `crates/docker`、`crates/git`、`crates/pty`：明确的未来原生边界，**未声明任何已实现能力**
- `plugins/*`：预留的插件包边界，**未实现**
- `scripts`：无法用根 pnpm / Cargo 命令表达的维护脚本（当前为 Windows 便携版打包）

## 文档

| 文档 | 内容 |
|---|---|
| [docs/architecture.md](docs/architecture.md) | 模块边界、前端分层、服务生命周期、各工作台的数据流 |
| [docs/development.md](docs/development.md) | 仓库结构、开发约定、测试布局与常见排错 |
| [docs/vault-security.md](docs/vault-security.md) | 密码库的威胁模型、密钥层级、文件格式、隔离保证，以及**明确不宣称**的能力 |
| [CHANGELOG.md](CHANGELOG.md) | 版本变更记录 |
| [CONTRIBUTING.md](CONTRIBUTING.md) | 提交前必须通过的检查与评审期望 |
| [SECURITY.md](SECURITY.md) | 漏洞报告方式与安全边界 |
| [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) | 社区行为准则 |

## 路线图

- Alpha 1（已完成）：服务编辑流程、进程树终止、更丰富的扫描元数据、发布产物
- Alpha 2（当前）：API Workbench、Database Workbench、Password Vault、中英文界面与 CSP 加固
- Alpha 3：密码库的独立安全评审、插件生命周期与第一方开发者工具扩展
- 后续：Docker/Git 工作流、MCP 工具，以及上下文原生的 AI

## 许可

[Apache License 2.0](LICENSE)。提交变更前请先阅读 [CONTRIBUTING.md](CONTRIBUTING.md)；安全问题请按 [SECURITY.md](SECURITY.md) 私下报告。
