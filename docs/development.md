# Development

This guide covers the day-to-day loop for working on Dev Workbench. See [architecture.md](architecture.md) for module boundaries and [CONTRIBUTING.md](../CONTRIBUTING.md) for review expectations.

## Prerequisites

- Node.js 22+ and pnpm 11+, pinned by [`.nvmrc`](../.nvmrc) and the root `packageManager` field
- Stable Rust with the `rustfmt` and `clippy` components, pinned by [`rust-toolchain.toml`](../rust-toolchain.toml)
- The [Tauri 2 system prerequisites](https://v2.tauri.app/start/prerequisites/) for your operating system

## First run

```bash
pnpm install
pnpm dev        # Vite dev server plus the Tauri desktop window
pnpm dev:web    # browser-only UI, no native shell
```

`pnpm dev` starts the Vite server on port 1420 with `strictPort`, then builds and launches the Rust shell. `pnpm dev:web` skips the native side, so anything that crosses the native bridge will not work there.

## Repository layout

| Path | Responsibility |
|---|---|
| `apps/desktop` | Vue views, router, Pinia stores, i18n, components, and the single Tauri bridge |
| `apps/desktop/src-tauri` | Rust application crate: commands, SQLite repository, project scanner, manifest parsers |
| `apps/desktop/src-tauri/migrations` | Versioned schema files applied once each and tracked in `_migrations` |
| `packages/shared` | Cross-boundary models, validation rules, and structured errors |
| `packages/command` | Command contracts, registry, execution, and fuzzy matching |
| `packages/context` | Observable workbench/project context |
| `packages/core` | Application services (`ProjectService`, `ServiceCatalog`, `ServiceManager`, `SystemService`, `SettingsService`, `ApiCatalog`, `VaultCatalog`), pure system and SQL-analysis helpers, the `NativeBridge` interface, and core command registration |
| `packages/ui` | Shared design tokens and future reusable primitives |
| `packages/plugin-api`, `packages/sdk` | Extension contracts kept deliberately small |
| `crates/process` | Managed child processes, process-tree termination, and log streaming |
| `crates/system`, `crates/network` | OS-facing process and port adapters |
| `crates/secrets` | OS credential-store boundary — implemented on Windows (Credential Manager), not yet on Linux or macOS |
| `crates/vault` | Password vault: cryptography, Argon2id parameters, page-locked memory, session, storage, and the threat-model suite |
| `crates/docker`, `crates/git`, `crates/pty` | Declared native boundaries with no implementation yet |
| `plugins/*` | Reserved plugin packages with no implementation yet |
| `docs/` | Architecture, development, and vault security documentation |

## Working rules

- Keep the dependency direction: `Vue UI → Application Service → Command System → Native Bridge → Tauri Command → Rust`.
- `apps/desktop/src/services/nativeBridge.ts` is the only module that calls `invoke`; views and components go through application services or commands instead.
- Add cross-boundary models to `packages/shared` first, and prefer registering a command over adding a view-specific code path.
- Validate a service in `packages/shared` for immediate UI feedback *and* in the Rust repository before persisting, so the database cannot be corrupted by a caller that skips the UI.
- Service working directories may be absolute or project-relative; the native runtime resolves relative and omitted directories against the owning project root before spawning.
- Keep operating-system differences inside the native adapter crates.
- Store only non-secret data in SQLite. Credentials and API keys belong to the OS keychain through `crates/secrets`; the password vault keeps its own encrypted database and must not be reachable from the service container.
- Treat `plugins/*`, `packages/plugin-api`, `packages/sdk`, and `crates/{docker,git,pty}` as ownership boundaries rather than shipped features, and do not describe them as implemented. `crates/secrets` and `crates/vault` are the exceptions: both are implemented, and both are security boundaries that need a deliberate design before they gain a caller or a capability.

## Tests

Vitest covers the TypeScript workspace; every package with a `test` script is picked up by `pnpm test` (223 cases today).

| Suite | Covers |
|---|---|
| `packages/shared` | Service validation rules, settings parsing, formatting helpers, path handling, structured errors, vault types, the URL scheme allow-list, and password strength and search |
| `packages/command` | Registry, execution, and fuzzy matching |
| `packages/context` | Observable context state |
| `packages/core` | Dependency ordering, batch start/stop semantics, catalog persistence, validation, process/port filtering and tree ordering, settings parsing, SQL script splitting and destructive-statement detection, and vault isolation (no command, context, plugin, or AI path can reach a secret) |
| `apps/desktop` | The workbench and system stores against a fake bridge and a real `createWorkbench` instance, the database workbench service and its locale-neutral runtime messages, the vault store's state clearing on every lock path, the utility tools, and i18n catalog parity |

Rust tests live next to the code they cover (196 cases today): `manifest` and `scanner` parse fixtures written to a temporary directory, `repository` exercises migrations and CRUD against a temporary SQLite file, the `process`, `system`, and `network` crates spawn real processes or parse recorded output, and `apps/desktop/src-tauri/src/{api,vault}.rs` cover the command layer's validation (URL schemes, backup extension normalisation).

`crates/vault` carries 144 of them. Its unit tests cover the AEAD boundary, Argon2id calibration, locked memory, the session, the generator, the model's redacted `Debug` implementations, and the unlock throttle; `crates/vault/tests/security.rs` is written against the threat model rather than the implementation, and [vault-security.md §10](vault-security.md#10-automated-security-tests) maps each guarantee to the test that enforces it.

`crates/system` cannot verify cross-process termination inside a restricted sandbox that denies opening processes it did not create; the test reports `SKIPPED` in that case instead of failing the suite.

## Checks

```bash
pnpm typecheck
pnpm test
pnpm build
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo check --workspace
```

CI (`.github/workflows/ci.yml`) runs the frontend checks plus `cargo fmt`, `cargo clippy`, and `cargo test` on every push to `main` and every pull request. On a `v*` tag, `.github/workflows/release.yml` builds installers for Linux, Windows, and macOS and opens a draft release; run `pnpm --filter @dev-workbench/desktop tauri build` locally to produce a bundle for your own platform.

## Data and generated files

- The SQLite database is created under the platform-specific Tauri application data directory; the schema is applied from `apps/desktop/src-tauri/migrations/` when the connection opens.
- `apps/desktop/src-tauri/gen/schemas` and `apps/desktop/dist` are generated and ignored by Git.

## Brand assets

`apps/desktop/src/assets/logo.png` is the canonical brand mark: the sidebar renders it, and `README.md` / `README.en.md` embed the same file so the public page cannot drift from the application. It is byte-identical to `apps/desktop/src-tauri/icons/128x128.png`, which `tauri-build` uses for the window and installer icons — change both together, and keep the alpha channel, because the mark is displayed on both light and dark backgrounds.

`apps/desktop/public/favicon.png` and `src-tauri/icons/icon.ico` / `icon.icns` are generated from the same artwork at smaller and platform-specific sizes. `build.rs` watches the icon directory, so replacing an icon actually reaches the executable and the taskbar.

## Troubleshooting

- `pnpm dev` fails before a window opens: verify the Tauri system prerequisites for your operating system.
- Type errors that disappear in a package-local run: run `pnpm typecheck` from the repository root so workspace references resolve consistently.
- Rust manifest or dependency changes: run `cargo check --workspace` once before `pnpm dev`, because the Tauri dev command builds the whole workspace.
