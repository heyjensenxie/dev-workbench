# Architecture

Dev Workbench follows one dependency direction: UI composition depends on application capabilities, while native implementation details remain behind `NativeBridge`. Commands receive a workbench snapshot and service container, so shortcuts, plugins, and future automation invoke the same operations as views.

SQLite owns durable non-secret project, service, and setting data. The Rust process runtime owns only processes launched by the app and emits line-oriented logs to the desktop window. OS-specific behavior is isolated inside native adapter crates.

The plugin API remains intentionally small in this release. Empty plugin and native capability packages define ownership, not implemented features.

## Frontend layers

| Layer | Responsibility |
|---|---|
| `apps/desktop/src/views` | Route-level composition only: read the store, wire components, own dialog state |
| `apps/desktop/src/components` | Presentational units: service list, service editor, project context, logs |
| `apps/desktop/src/composables` | Behaviour shared by more than one view, such as the service workspace |
| `apps/desktop/src/stores` | Pinia stores holding view-facing state and translating user intent into application-service calls |
| `packages/core` | `ProjectService`, `ServiceCatalog`, `ServiceManager`, `SystemService`, `SettingsService`, the `NativeBridge` contract, and core commands |
| `packages/context` | Observable active-project/workbench state shared by commands and views |

`NativeBridge` is the only abstraction that talks to Rust, and `apps/desktop/src/services/nativeBridge.ts` is the only module that calls `invoke`. Everything above it stays testable in Node, which is how `apps/desktop/src/stores/*.test.ts` exercise the stores against a fake bridge and a real `createWorkbench` instance.

Three stores split the view state: `workbench` owns projects, services, and logs; `system` owns the process and port surfaces; `settings` owns the persisted preferences. `system` delegates all of its ordering, filtering, and tree building to pure helpers in `packages/core`, so the interesting logic is unit-tested without a DOM.

## System surfaces

`SystemService` is a thin pass-through to the OS adapters: process listing, process-tree lookup, tree termination, port listing, and port release. The interesting behaviour lives in pure functions — `matchesProcess`, `matchesPort`, `sortProcessesByMemory`, and `treeOrder`/`processDepths`, which build parent/child order and nesting depth with cycle protection so a corrupt parent link cannot hang the view.

## Settings

Preferences live in the `settings` table as JSON values keyed by name, read and written through `get_settings` / `set_setting` and validated on the Rust side (non-empty, whitespace-free, at most 64 characters). `parseSettings` in `packages/shared` keeps only the persisted fields that are present and usable, so the desktop store keeps its own default for anything missing or corrupt. Theme and locale are applied immediately from `localStorage` at boot and then reconciled with the database, which keeps startup instant while still syncing preferences across runs.

## Service lifecycle

`ServiceCatalog` validates and normalizes a service before it crosses the bridge; the Rust repository validates it again, trims it, and enforces a unique name per project. `ServiceManager` owns runtime state: it orders services by their dependencies for start-up, reverses that order for shutdown, and skips dependents whose dependency failed instead of reporting the same failure twice. `list_running_services` lets the UI reconcile its state with processes the runtime still owns, and `service-exit` reports a service that ended on its own.

Service working directories are portable: an absolute directory is used as-is,
while a relative directory is resolved against the persisted project root. An
omitted directory also defaults to that root, so scanner suggestions always run
in the project they came from.

## Process ownership

`crates/process` owns every process the app starts, together with its log readers. Stopping a service sweeps the whole process tree so build and watcher children cannot keep holding a port:

- Windows: `taskkill /PID <pid> /T /F`, because the OS parent chain is the only reliable way to reach grandchildren.
- Unix: the service is spawned into its own process group and signalled with `kill -TERM -<pgid>`, escalating to `-KILL` after the grace period.

If a platform denies tree termination, the runtime still stops the service through the child handle it owns, and aborts its log readers instead of waiting on inherited pipe handles.

## Data and migrations

`repository::connect` opens SQLite with foreign keys, WAL journaling, and a busy timeout, then applies each schema file in `apps/desktop/src-tauri/migrations/` at most once, recording it in `_migrations`. Every migration runs in a transaction, so a release can add columns without dropping user data.

## Scanner

`scanner` reads only well-known manifests. `manifest` holds the pure parsers (package.json, compose, Makefile, Cargo, `.git/HEAD`) so their behaviour is unit-tested without touching a project directory. The scanner reads the project root plus one level of `apps/*`, `packages/*`, `plugins/*`, and `crates/*` for monorepos, and turns what it finds into stack metadata, detected files, scripts, and importable service suggestions.
