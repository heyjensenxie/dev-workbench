# Changelog

All notable changes are documented here.

## Unreleased

### Changed

- Application icons, installer icons, and the in-app brand mark now use the Dev Workbench logo; the browser UI gained a proper document head with a favicon
- The sidebar now has real pages for Services, Processes, and Ports instead of a redirect, and the app persists theme, language, log retention, and kill confirmation through the `settings` table, with `localStorage` kept as the instant-boot fallback

### Added

- Processes page: searchable list with memory, parent, and start time, memory or process-tree ordering, and tree termination
- Ports page: listening ports with the owning process and one-click release
- Settings page with appearance, runtime, and about sections, plus `get_settings` / `set_setting` commands storing JSON values
- Process metadata now includes the executable path, resident memory, and start time
- Pure system helpers in `packages/core`: process/port matching, memory ordering, and parent/child tree ordering with cycle safety
- Service editing flow: create, edit, and delete services with arguments, environment variables, working directory, port, auto-open, and dependency selection
- Service list with live status, PID, per-service start/stop/restart, and dependency-order start-all/stop-all that skips dependents of a failed service
- Project scanner metadata: manifest and framework detection, `package.json` scripts, Docker Compose services, Makefile targets, monorepo workspace discovery, and the current Git branch
- One-click import of scanner-produced service suggestions
- Log panel with per-service filtering, clearing, and auto-scroll
- Process-tree termination when a service is stopped or a process/port is killed
- `list_running_services`, `stop_all_services`, and `service-exit` events so the UI reconciles with the native runtime
- Versioned SQLite migrations tracked in `_migrations`, applied once and inside a transaction
- Frontend unit tests for `packages/shared`, `packages/command`, `packages/context`, `packages/core`, and the desktop store
- `release.yml` workflow that bundles Linux, Windows, and macOS installers into a draft release on `v*` tags

### Fixed

- SQLite now enables foreign keys per connection, so deleting a project cascades to its services
- Service names are trimmed and unique per project, and invalid definitions are rejected before they reach the database
- Log readers are aborted on stop instead of lingering on pipe handles inherited by surviving grandchild processes
- The desktop route now selects the project named in `/projects/:id`
- Replacing the application icon now actually changes the executable, window, and taskbar icon: `tauri-build` embeds `icons/icon.ico` without declaring it as a build input, so `build.rs` watches the icon directory and the Tauri configuration

## 0.1.0-alpha.1

- Initial Tauri and Vue desktop workbench architecture
- Project persistence and stack scanning
- Command, context, service-runtime, log-streaming, process, and port foundations
- Light/dark developer-tool shell and command palette

