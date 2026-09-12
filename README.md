# Dev Workbench

[中文](README.md) | [English](README.en.md)

> 面向现代软件开发、本地优先且可扩展的开发者工作台。

Dev Workbench 是围绕项目构建的桌面环境，用于理解和运行本地软件项目。它不堆砌互不相关的工具，而是把项目、服务、进程、端口、日志与命令上下文收敛在稳定的应用边界之后，供 UI、插件、快捷键以及未来的 AI 集成共同使用。

当前版本：**v0.1.0-alpha.1**

## 当前能力

- Tauri 2 + Vue 3 桌面外壳，支持明暗主题与中英文界面；首次启动的语言跟随操作系统
- 项目数据持久化到本地 SQLite 数据库，按版本一次性应用并记录迁移
- Projects 页面支持添加、打开、刷新、在文件管理器中显示，以及安全移除工作台记录（不会删除磁盘文件）
- 项目扫描器：识别语言、包管理器、框架、清单/锁文件、容器与 CI 配置、Agent 指令，并解析 `package.json`、`Cargo.toml`、compose 文件与 `Makefile`
- 基于扫描结果给出可一键导入的服务建议（脚本、compose 服务、make 目标），并读取当前 Git 分支
- 服务管理闭环：新增、编辑、删除服务，编辑参数、环境变量、工作目录、端口与依赖关系
- 服务启动、停止、重启，按依赖顺序的全部启动与全部停止；依赖启动失败时自动跳过下游服务
- 托管本地子进程，终止时连同子进程树一起清理，避免残留进程占用端口
- 运行中的服务状态与 PID 实时同步，服务自行退出时界面会收到事件
- 进程页：可筛选的进程列表（内存、父进程、启动时间），支持按内存或进程树排序，并可直接结束进程树
- 端口页：监听端口与占用进程，一键释放
- 服务配置端口与监听端口自动关联，并在启动前提示端口占用者
- 设置页：系统/深色/浅色主题、语言、启动时恢复上次项目、日志保留行数与「结束进程前确认」，持久化到本地数据库
- Utilities 页面：本地 JSON 格式化/压缩、JWT 解码、时间戳转换、UUID 生成、Base64 与 URL 编解码、SHA-1/SHA-256 Hash 与正则测试
- 跨平台的进程清单（含父进程与内存）与监听端口探测、进程名解析
- 按服务过滤、搜索、暂停/恢复自动滚动、清空并高亮 ERROR/WARN 的运行时日志面板
- 共享上下文存储与类型化命令注册表，`Ctrl+K` 模糊匹配命令面板

本 alpha 版本**不包含** HTTP 客户端、数据库客户端、Redis 工具、完整的 Docker 或 Git UI、MCP Inspector、AI、插件市场、Kubernetes 或 SSH 支持。

## 技术栈

- 桌面端：Tauri 2、Rust、Tokio、Serde
- 前端：Vue 3、TypeScript（strict）、Vite、Pinia、Vue Router
- UI：Tailwind CSS 4、Reka UI、Lucide 图标
- 持久化：SQLite 与 SQLx
- 工作区：pnpm 与 Cargo workspace

## 开发

前置条件：Node.js 22+、pnpm 11+、stable Rust 工具链，以及对应操作系统的 [Tauri 系统依赖](https://v2.tauri.app/start/prerequisites/)。Node 与 Rust 版本分别由 [`.nvmrc`](.nvmrc) 和 [`rust-toolchain.toml`](rust-toolchain.toml) 固定。

```bash
pnpm install
pnpm dev
```

仅开发浏览器端界面：

```bash
pnpm dev:web
```

质量检查：

```bash
pnpm typecheck
pnpm test
pnpm build
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo check --workspace
```

`pnpm test` 在每个包含测试的工作区包中运行 Vitest（`packages/shared`、`packages/command`、`packages/context`、`packages/core`、`apps/desktop`），`cargo test --workspace` 覆盖原生适配器与桌面后端。

生成当前平台的安装包：

```bash
pnpm --filter @dev-workbench/desktop tauri build
```

推送 `v*` 标签时，[`.github/workflows/release.yml`](.github/workflows/release.yml) 会为 Linux、Windows 与 macOS 构建安装包并创建草稿 Release 等待人工确认。

应用数据保存在各平台 Tauri 应用数据目录下。凭据与 API Key 不得写入 SQLite；`crates/secrets` 边界为未来的 OS Keychain 实现保留。

完整的开发指南——仓库结构、开发约定与常见排错——见 [docs/development.md](docs/development.md)。

## 架构

```text
Vue UI → Application Service → Command System → Native Bridge → Tauri Command → Rust
```

- `apps/desktop`：桌面端组合、路由、视图，以及唯一的 Tauri 桥接实现
- `packages/shared`：跨边界模型与结构化错误
- `packages/command`：命令契约、注册表、执行与模糊匹配
- `packages/context`：可观察的工作台/项目上下文
- `packages/core`：应用服务与核心命令注册
- `packages/ui`：共享设计令牌与未来的可复用基础组件
- `packages/plugin-api`、`packages/sdk`：刻意保持精简的扩展契约
- `crates/process`：托管子进程运行时、进程树终止与日志流
- `crates/system`、`crates/network`：面向操作系统的进程与端口适配器
- `crates/docker`、`crates/git`、`crates/secrets`、`crates/pty`：明确的未来原生边界，未声明任何已实现能力
- `plugins/*`：预留的插件包边界；被排除的能力仍未实现

## 路线图

- Alpha 1（已完成）：服务编辑流程、进程树终止、更丰富的扫描元数据、UI 测试与发布产物
- Alpha 2：插件生命周期与第一方开发者工具扩展
- 后续：HTTP、数据存储、更完整的 Docker/Git 工作流、MCP 工具，以及上下文原生的 AI

提交变更前请先阅读 [CONTRIBUTING.md](CONTRIBUTING.md)。安全问题的报告方式见 [SECURITY.md](SECURITY.md)。
