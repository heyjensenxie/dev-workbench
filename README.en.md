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

> A local-first, extensible developer workbench for modern software development.

Dev Workbench is a project-centric desktop environment for understanding and running local software projects. Instead of collecting unrelated utilities, it keeps project, service, process, port, log, and command context behind stable application boundaries that the UI, plugins, shortcuts, and future AI integrations can share.

Current version: **v0.1.0-alpha.2** (pre-release; bundles are unsigned)

## Design stance

Three decisions shape the project, and they explain why it is not another tool collection:

1. **Local-first, and offline by default.** Project records, service definitions, settings, saved API requests, and database connections live in local SQLite. The only thing that leaves the machine is an HTTP request you explicitly send.
2. **Credentials are physically separated from ordinary data.** Database passwords go to the OS credential store, and the Password Vault is a fully independent encryption domain — its own `vault.db`, its own key hierarchy, and deliberately absent from the shared service container, so no plugin or future AI surface can reach it.
3. **A capability that cannot be delivered is not written into the docs.** `crates/{docker,git,pty}` and `plugins/*` are **ownership boundaries with no implementation**. The README and `docs/` do not describe them as shipped features.

## Current capabilities

### Projects and services

- Tauri 2 + Vue 3 desktop shell with light/dark themes and Chinese/English UI; the initial locale follows the operating system
- Project persistence in a local SQLite database, with versioned migrations applied once and recorded
- Projects page for adding, opening, refreshing, revealing in the file manager, and safely removing workbench records without touching disk files
- Project scanner for languages, package managers, frameworks, manifests and lockfiles, container and CI configuration, and agent instructions; it parses `package.json`, `Cargo.toml`, compose files, and `Makefile`
- Service suggestions derived from the scan (scripts, compose services, make targets) that import with one click, plus the current Git branch
- Complete service editing: create, edit, and delete services with arguments, environment variables, working directory, port, and dependencies
- Service start, stop, and restart, dependency-order start-all and stop-all, and automatic skipping of dependents when a dependency fails to start
- Managed local child processes terminated together with their process tree, so nothing is left holding a port
- Live service state and PID tracking, with an event when a service exits on its own

### Processes, ports, and logs

- Processes page: a searchable process list (memory, parent, start time) ordered by memory or by process tree, with tree termination
- Ports page: listening ports with the owning process and one-click release
- Configured service ports are linked to listening ports, with a preflight error naming the process that already owns a port
- Runtime log panel with per-service filtering, search, pause/resume auto-scroll, clearing, and ERROR/WARN highlighting
- Cross-platform process inventory (parent pid and memory) plus listening-port detection and process-name resolution

### API Workbench (HTTP client)

- Request composer with query parameters, headers, and a JSON body, plus method, timeout, and `Ctrl / ⌘ + Enter` to send
- Saved request templates grouped into modules, and recent-request history whose URLs are redacted first (credentials stripped, token-like query values replaced) before they are shown
- Environment variables live only in session memory and are resolved into `{{VAR}}` templates just before sending, so saved rows never hold a secret
- Response inspection: status, duration, size, and formatted / raw / headers views

### Database Workbench (database client)

- MySQL and SQLite connection profiles; the database password is written to the OS credential store and the connection record keeps only a `secretRef` marker
- Schema tree and a multi-tab SQL editor
- Result, message, execution-plan, structure, DDL, and query-history tabs
- Destructive statements (`DROP`, `TRUNCATE`, `ALTER TABLE`, `UPDATE`/`DELETE` without `WHERE`) are analysed locally and confirmed before they run
- SQL analysis lives in `packages/core` as pure helpers — statement splitting that respects quoted semicolons, read-query detection, and result limits — so the safety rules are unit-tested without a real database

### Password Vault (local credential store)

- An independent encryption domain: the master password derives a KEK with Argon2id, the KEK only wraps a random 256-bit vault master key (VMK), and every record is encrypted with XChaCha20-Poly1305 into a separate `vault.db`
- The schema has **no** `title`, `username`, `password`, `url`, or `notes` column: the entire record body is serialised and encrypted as one payload, leaving only an opaque id, an opaque nonce, ciphertext, and timestamps
- Item CRUD, tags, favourites, sensitive custom fields, and in-memory search over decrypted summaries
- A CSPRNG password generator, and clipboard auto-clear that only removes a value still belonging to us
- Encrypted `.vaultbackup` export and restore, where restore validates the header, the wrapped key, and every record before writing anything, and writes a timestamped safety copy of the current vault first
- Master-password change, encryption-strength re-benchmarking, auto-lock on idle, exit, lock screen, machine sleep, window hide, and `Ctrl + Shift + L`, plus progressive unlock throttling
- An automated threat-model suite, `crates/vault/tests/security.rs`, asserting that no plaintext — master password, username, password, note, title, URL, tag, or custom field — ever reaches the vault file, its write-ahead log, or its sidecar files

### Application shell

- Utilities page: local JSON formatting/minifying, JWT decoding, timestamp conversion, UUID generation, Base64 and URL component encoding/decoding, SHA-1/SHA-256 hashing, regex testing, plus SQL format/minify/IN builder/parameter fill/DDL preview and MyBatis log restoration
- Settings page for system/light/dark theme, language, restoring the last project on startup, log retention, and kill confirmation, persisted to the local database
- Shared context store and typed command registry behind a `Ctrl+K` fuzzy command palette
- Production builds ship a strict CSP: scripts and styles are limited to bundled assets, connections are allowed only to the Tauri IPC and self, and objects, framing, and form submission are forbidden

## Platform support and known limitations

This is an alpha. The limitations below are **known and deliberately published** — judge for yourself whether it fits your use:

| Capability | Windows | Linux | macOS |
|---|---|---|---|
| Projects / services / processes / ports / logs | ✅ | ✅ | ✅ |
| API Workbench | ✅ | ✅ | ✅ |
| Database Workbench (queries and schema tree) | ✅ | ✅ | ✅ |
| Database password in the OS credential store | ✅ Windows Credential Manager | ⚠️ not implemented | ⚠️ not implemented |
| Password Vault | ✅ | ✅ | ✅ |
| Vault key page-locking (anti-swap) | ✅ `VirtualLock` | ✅ `mlock` | ✅ `mlock` |
| Clipboard auto-clear | ✅ | ⚠️ see below | ⚠️ see below |

- **`crates/secrets` is Windows-only today.** On Linux and macOS, saving a database password fails with an explicit "OS secret storage is not implemented on this platform" rather than silently dropping it; querying is unaffected.
- **Clipboard auto-clear depends on a native implementation.** Where there is none, the command fails loudly and the UI says automatic clearing is unavailable rather than pretending it succeeded. Manual copying still works.
- **The Password Vault has not had an independent security review.** It is implemented and covered by an automated threat-model suite, but per [docs/vault-security.md](docs/vault-security.md) §11 it still requires an independent review before a public Beta. Until then, use it **on your own machine only** — not for shared or production credentials.
- **There is no password recovery.** No server, no account, no recovery key, and no backdoor. Losing the master password means losing the vault.
- **Bundles are unsigned.** The operating system may require an explicit "open anyway" confirmation. Windows installers, macOS `.dmg`, and Linux `.deb`/`.AppImage` are built as a **draft** release by `release.yml` and published only after manual confirmation.
- **Data location**: application data — including `workbench.sqlite3` and the separate `vault.db` — is stored under the platform-specific Tauri application data directory.

## Not in this release

Redis tools, a full Docker or Git UI, MCP Inspector, AI features, a plugin marketplace, Kubernetes, or SSH support. The corresponding directories (`crates/{docker,git,pty}`, `plugins/*`) are reserved ownership boundaries with no implementation, and are never described as delivered capabilities.

## Technology

| Layer | Choice | What it does here |
|---|---|---|
| Desktop shell | Tauri 2 + Rust (edition 2024), Tokio, Serde | Window, IPC, and every native capability. It reuses the system WebView instead of embedding Chromium, which is why the bundle is far smaller than an Electron equivalent |
| Frontend | Vue 3 + TypeScript (strict) + Vite | Views, routing, and interaction. `vue-tsc` runs in both `build` and CI — type checking is not optional |
| State and routing | Pinia + Vue Router (hash mode) | Four stores hold workbench, system, settings, and vault view state |
| UI | Tailwind CSS 4, Reka UI, Lucide | Design tokens and unstyled accessible components; styling stays inside `apps/desktop` and does not leak into other packages |
| Persistence | SQLite + SQLx (async) | Projects, services, settings, saved API requests, and database connection profiles; versioned migrations apply once inside a transaction and are recorded |
| Cryptography | RustCrypto `argon2` and `chacha20poly1305` | The vault's KDF and AEAD. No custom construction, no XOR scheme, no unauthenticated CBC |
| Native adapters | `crates/{process,system,network}` | Process-tree management, process inventory, and port detection. Operating-system differences exist only inside these three crates |
| Workspace | pnpm and Cargo workspaces | 13 frontend packages and 9 Rust crates, with one lockfile each |

## How it works

**The dependency direction is one-way.** `Vue UI → Application Service → Command System → Native Bridge → Tauri Command → Rust`. Exactly one file in the whole frontend calls `invoke` — `apps/desktop/src/services/nativeBridge.ts` — and everything else goes through an application service or the command registry. What that buys is testability: the layers above can run in Node against a fake bridge and a real `createWorkbench` instance, which is exactly how `apps/desktop/src/stores/*.test.ts` work.

**The native surface is bounded.** Every native capability is one of the 56 Tauri commands registered in `apps/desktop/src-tauri/src/lib.rs`, 23 of which belong to the vault. The vault adds two hard constraints on top: it is not registered in the shared service container (so plugins cannot resolve it), and it exposes only `vault.open` and `vault.lock`. A new native capability has to be added to that list deliberately; it cannot appear by accident.

**Process ownership is explicit.** The app manages only the processes it started. Stopping a service sweeps the whole tree — on Windows with `taskkill /T /F`, because the parent/child chain recorded by the OS is the only reliable way to reach grandchildren; on Unix the service is spawned into its own process group and signalled with `kill -TERM -<pgid>`, escalating to `-KILL` after the grace period. Log readers are aborted on stop, because a pipe handle inherited by a surviving grandchild would otherwise keep a reader alive indefinitely.

**Data lands in three separate places, each for a reason.**

| Location | Holds | Why it is separate |
|---|---|---|
| `workbench.sqlite3` | Projects, services, settings, saved API requests, database connection profiles | Ordinary application data, with one read/write path shared by several features |
| `vault.db` | The Password Vault | The schema has no title, username, password, URL, or notes column, so whatever is written is already ciphertext — which is why the write-ahead log and sidecar files can only ever hold ciphertext too |
| OS credential store | Database connection passwords | The application database keeps only a `secretRef` marker, so the password itself never touches disk |

**Cross-platform differences are quarantined in the adapters.** `crates/system`, `crates/network`, and `crates/process` each provide a Windows and a Unix implementation, and nothing else writes a platform check. That rule is also what makes `crates/secrets` honest at a glance: it has a Windows implementation only, and the other platforms fail explicitly rather than degrading silently.

**Security is narrowed by default rather than hardened item by item.** Production builds ship a strict CSP (scripts and styles limited to bundled assets, objects, framing, and form submission forbidden); API request URLs accept only `http`/`https`; and destructive database statements are analysed locally and confirmed before they run.

## Getting started

### Run a released bundle

Download the installer for your platform from [Releases](https://github.com/heyjensenxie/dev-workbench/releases). The bundles are unsigned, so the operating system may ask you to confirm before opening one.

### Run from source

Prerequisites: Node.js 22+, pnpm 11+, the stable Rust toolchain, and the [Tauri system prerequisites](https://v2.tauri.app/start/prerequisites/) for your operating system. Node and Rust versions are pinned by [`.nvmrc`](.nvmrc) and [`rust-toolchain.toml`](rust-toolchain.toml).

```bash
pnpm install
pnpm dev          # Vite dev server plus the Tauri desktop window
pnpm dev:web      # browser-only UI, no native shell
```

Build an installer for the current platform (MSI / NSIS, for distribution):

```bash
pnpm --filter @dev-workbench/desktop tauri build
```

Build the install-free portable Windows executable (recommended for your own machine):

```bash
pnpm build:portable
```

The result is `release/Dev Workbench.exe`. Double-click it to run — there is no installation and no administrator prompt, and the only external prerequisite is the Microsoft Edge WebView2 runtime that ships with Windows 10 and 11. The file name carries no version, so a pinned taskbar entry survives a rebuild. See [docs/development.md](docs/development.md#packaging).

## Quality checks

```bash
pnpm typecheck
pnpm test
pnpm build
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo check --workspace
```

These are exactly what [`.github/workflows/ci.yml`](.github/workflows/ci.yml) runs on every push to `main` and every pull request. 223 Vitest cases across the frontend (`packages/{shared,command,context,core}`, `apps/desktop`) and 196 Rust cases (144 of them in `crates/vault`) cover pure logic, storage, parsers, and the security boundaries.

## Releases

Pushing a `v*` tag makes [`.github/workflows/release.yml`](.github/workflows/release.yml) build installers for Linux, Windows, and macOS (both Apple Silicon and Intel targets) and open a **draft** release for review.

## Repository layout

- `apps/desktop`: desktop composition, routes, views, and the only concrete Tauri bridge
- `packages/shared`: cross-boundary models, validation rules, and structured errors
- `packages/command`: command contracts, registry, execution, and fuzzy matching
- `packages/context`: observable workbench/project context
- `packages/core`: application services (project, service catalog, service manager, system, settings, API, database) and core command registration
- `packages/ui`: shared design tokens and future reusable primitives
- `packages/plugin-api`, `packages/sdk`: deliberately small extension contracts
- `crates/process`: managed child-process runtime, process-tree termination, and log streaming
- `crates/system`, `crates/network`: OS-facing process and port adapters
- `crates/secrets`: the OS credential-store boundary (Windows implementation only today)
- `crates/vault`: the vault's cryptography, KDF, page-locked memory, session, and storage
- `crates/docker`, `crates/git`, `crates/pty`: explicit future native boundaries, with **no claimed implementation**
- `plugins/*`: reserved plugin package boundaries, **unimplemented**
- `scripts`: maintenance scripts that cannot be expressed as a root pnpm or Cargo command (today, portable Windows packaging)

## Documentation

| Document | Contents |
|---|---|
| [docs/architecture.md](docs/architecture.md) | Module boundaries, frontend layers, service lifecycle, and each workbench's data flow |
| [docs/development.md](docs/development.md) | Repository layout, working rules, test layout, and troubleshooting |
| [docs/vault-security.md](docs/vault-security.md) | The vault's threat model, key hierarchy, file format, isolation guarantees, and an explicit list of what it does **not** claim |
| [CHANGELOG.md](CHANGELOG.md) | Release history |
| [CONTRIBUTING.md](CONTRIBUTING.md) | The checks a change must pass and review expectations |
| [SECURITY.md](SECURITY.md) | How to report a vulnerability, and the security boundaries |
| [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) | Community expectations |

## Roadmap

- Alpha 1 (done): service editing flows, process-tree termination, richer scanner metadata, release assets
- Alpha 2 (current): API Workbench, Database Workbench, Password Vault, Chinese/English UI, and CSP hardening
- Alpha 3: independent security review of the vault, plugin lifecycle, and first-party developer-tool extensions
- Later: Docker/Git workflows, MCP tooling, and context-native AI

## License

[Apache License 2.0](LICENSE). See [CONTRIBUTING.md](CONTRIBUTING.md) before submitting changes; report security issues privately per [SECURITY.md](SECURITY.md).
