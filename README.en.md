# Dev Workbench

[中文](README.md) | [English](README.en.md)

> A local-first, extensible developer workbench for modern software development.

Dev Workbench is a project-centric desktop environment for understanding and running local software projects. Instead of collecting unrelated utilities, it keeps project, service, process, port, log, and command context behind stable application boundaries that the UI, plugins, shortcuts, and future AI integrations can share.

Current version: **v0.1.0-alpha.1**

## Current capabilities

- Tauri 2 + Vue 3 desktop shell with light/dark themes and Chinese/English UI; the initial locale follows the operating system
- Project persistence in a local SQLite database, with versioned migrations applied once and recorded
- Projects page for adding, opening, refreshing, revealing in the file manager, and safely removing workbench records without touching disk files
- Project scanner for languages, package managers, frameworks, manifests and lockfiles, container and CI configuration, and agent instructions; it parses `package.json`, `Cargo.toml`, compose files, and `Makefile`
- Service suggestions derived from the scan (scripts, compose services, make targets) that import with one click, plus the current Git branch
- Complete service editing: create, edit, and delete services with arguments, environment variables, working directory, port, and dependencies
- Service start, stop, and restart, dependency-order start-all and stop-all, and automatic skipping of dependents when a dependency fails to start
- Managed local child processes terminated together with their process tree, so nothing is left holding a port
- Live service state and PID tracking, with an event when a service exits on its own
- Processes page: a searchable process list (memory, parent, start time) ordered by memory or by process tree, with tree termination
- Ports page: listening ports with the owning process and one-click release
- Configured service ports are linked to listening ports, with a preflight error naming the process that already owns a port
- Settings page for system/light/dark theme, language, restoring the last project on startup, log retention, and kill confirmation, persisted to the local database
- Utilities page with local JSON formatting/minifying, JWT decoding, timestamp conversion, UUID generation, Base64 and URL component encoding/decoding, SHA-1/SHA-256 hashing, and regex testing
- Cross-platform process inventory (parent pid and memory) plus listening-port detection and process-name resolution
- Runtime log panel with per-service filtering, search, pause/resume auto-scroll, clearing, and ERROR/WARN highlighting
- Shared context store and typed command registry behind a `Ctrl+K` fuzzy command palette

This alpha does **not** include an HTTP client, database client, Redis tools, a full Docker or Git UI, MCP Inspector, AI, marketplace, Kubernetes, or SSH support.

## Technology

- Desktop: Tauri 2, Rust, Tokio, Serde
- Frontend: Vue 3, TypeScript (strict), Vite, Pinia, Vue Router
- UI: Tailwind CSS 4, Reka UI, Lucide icons
- Persistence: SQLite and SQLx
- Workspace: pnpm and Cargo workspaces

## Development

Prerequisites: Node.js 22+, pnpm 11+, the stable Rust toolchain, and the [Tauri system prerequisites](https://v2.tauri.app/start/prerequisites/) for your operating system. Node and Rust versions are pinned by [`.nvmrc`](.nvmrc) and [`rust-toolchain.toml`](rust-toolchain.toml).

```bash
pnpm install
pnpm dev
```

For browser-only UI development:

```bash
pnpm dev:web
```

Quality checks:

```bash
pnpm typecheck
pnpm test
pnpm build
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo check --workspace
```

`pnpm test` runs Vitest in every workspace package that has tests (`packages/shared`, `packages/command`, `packages/context`, `packages/core`, `apps/desktop`), while `cargo test --workspace` covers the native adapters and the desktop backend.

Build an installer for the current platform:

```bash
pnpm --filter @dev-workbench/desktop tauri build
```

Pushing a `v*` tag makes [`.github/workflows/release.yml`](.github/workflows/release.yml) bundle Linux, Windows, and macOS installers and open a draft release for review.

Application data is stored under the platform-specific Tauri application data directory. Credentials and API keys must never be stored in SQLite; the `crates/secrets` boundary is reserved for an OS keychain implementation.

The full development guide — repository layout, working rules, and troubleshooting — is in [docs/development.md](docs/development.md).

## Architecture

```text
Vue UI → Application Service → Command System → Native Bridge → Tauri Command → Rust
```

- `apps/desktop`: desktop composition, routes, views, and the only concrete Tauri bridge
- `packages/shared`: cross-boundary models and structured errors
- `packages/command`: command contracts, registry, execution, and fuzzy matching
- `packages/context`: observable workbench/project context
- `packages/core`: application services and core command registration
- `packages/ui`: shared design tokens and future reusable primitives
- `packages/plugin-api`, `packages/sdk`: deliberately small extension contracts
- `crates/process`: managed child-process runtime, process-tree termination, and log streaming
- `crates/system`, `crates/network`: OS-facing process and port adapters
- `crates/docker`, `crates/git`, `crates/secrets`, `crates/pty`: explicit future native boundaries, with no claimed feature implementation
- `plugins/*`: reserved plugin package boundaries; excluded features remain unimplemented

## Roadmap

- Alpha 1 (done): service editing flows, process-tree termination, richer scanner metadata, UI tests, and release assets
- Alpha 2: plugin lifecycle and first-party developer-tool extensions
- Later: HTTP, data stores, richer Docker/Git workflows, MCP tooling, and context-native AI

See [CONTRIBUTING.md](CONTRIBUTING.md) before submitting changes. Security reports should follow [SECURITY.md](SECURITY.md).
