# Changelog

All notable changes are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html). Pre-1.0
releases are marked with pre-release identifiers (`-alpha.N`) because the plugin
and native boundaries described in [docs/architecture.md](docs/architecture.md)
are still moving.

## Unreleased

### Added

- `pnpm build:portable` and [`scripts/build-portable.ps1`](scripts/build-portable.ps1) produce a self-contained Windows executable at `release/Dev Workbench.exe`: no installer, no administrator rights, and no side-by-side files. The output name carries no version so a pinned shortcut survives a rebuild; `-OutDir` and `-Configuration` are available, and the script targets Windows PowerShell 5.1 so PowerShell 7 is not required
- `docs/development.md` gained a Packaging section covering both build outputs, the portable executable's WebView2 prerequisite, and where application data lives relative to the binary

### Changed

- The root `Cargo.toml` now declares a size-first `[profile.release]` — fat LTO, one codegen unit, `opt-level = "s"`, `panic = "abort"`, and stripped symbols — which brings the portable executable to about 7 MB; the tradeoff is a slower cold release build

### Fixed

- Release builds no longer open a console window next to the application. `apps/desktop/src-tauri/src/main.rs` was missing the `windows_subsystem = "windows"` attribute that the Tauri template ships, so the linker emitted a console-subsystem executable (PE subsystem 3) and Windows attached a `cmd` window to every release launch — most visibly to the double-clicked portable build. Debug builds keep the console so `pnpm dev` still shows the runtime's output

## 0.1.0-alpha.2 - 2026-09-13

The first public release. This section collects everything built on top of the
`0.1.0-alpha.1` architecture foundations.

### Changed

- SQL safety analysis in `packages/core` now reports locale-neutral warning kinds instead of English copy, so any interface language can render `DROP`, `TRUNCATE`, `ALTER TABLE`, `UPDATE without WHERE`, and `DELETE without WHERE` findings
- Application icons, installer icons, and the in-app brand mark now use the Dev Workbench logo; the browser UI gained a proper document head with a favicon
- The sidebar now has real pages for Services, Processes, and Ports instead of a redirect, and the app persists theme, language, log retention, and kill confirmation through the `settings` table, with `localStorage` kept as the instant-boot fallback
- The WebView Content Security Policy is no longer `null`: production builds now restrict scripts and styles to bundled assets, allow connections only to the Tauri IPC and self, and forbid objects, framing, and form submission; development keeps a separate relaxed policy for the Vite HMR socket

### Added

- Database Workbench: MySQL and SQLite connection profiles with passwords kept in the OS credential store, a schema tree, multi-tab SQL editing with confirmation before destructive statements, and result, message, execution-plan, structure, DDL, and history tabs
- API Workbench: an HTTP request composer with query, header, and body editors, session-only environment variables, saved request templates grouped into modules, recent-request history with redacted URLs, and response inspection
- Chinese and English interface for the Database and API workbenches, their sidebar entries, and the command palette; the native runtime and the browser preview now describe their own output with locale-neutral codes that `apps/desktop/src/i18n.ts` translates, while driver and engine errors keep their original wording
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
- Password Vault (`workbench-vault`, `/vault`): an isolated, local-first credential store where the master password derives an Argon2id KEK that only wraps a random 256-bit vault master key, and every record is encrypted with XChaCha20-Poly1305 before it reaches its own `vault.db`; the vault database has no column for a title, username, password, URL, or note, so a stolen file yields ciphertext only
- Password Vault features: create/unlock/lock, item CRUD with tags, favourites and sensitive custom fields, in-memory search over decrypted summaries, a CSPRNG password generator, clipboard auto-clear that only removes a value still belonging to us, encrypted `.vaultbackup` export *and* restore, master-password change, and an implemented-but-not-yet-exposed vault key rotation
- Restoring an encrypted backup lives in the settings dialog (and on the lock screen) rather than only on the create-vault screen, because a user who already has a vault never sees the create screen — the previous placement made the feature invisible to exactly the people who needed it; restoring while a vault exists replaces it, and the native layer writes a timestamped safety copy of the current vault first so a wrong choice can be undone
- "Remove local vault" closes the two dead ends that restriction created — a backup restored with a password you no longer have, and wanting to replace an existing vault — by deleting every record and the wrapped key, checkpointing the write-ahead log, and rebuilding the file; it is reachable from both the lock screen and the settings dialog, and is guarded by a typed confirmation rather than the master password, because deleting a local file never required one and prompting for it would have blocked the very case that needs this
- Unlock throttling: five consecutive failed attempts lock the vault out for 5 minutes, each further failure doubles that up to a one-hour ceiling, and only a successful unlock resets the counter — so an expired lockout still escalates the next failure; the counter is persisted in the vault header rather than held in memory, because throttling a restart clears is not throttling, and every path that verifies the master password (unlock, change-password, key rotation, re-benchmarking) shares it so the lockout cannot be side-stepped by guessing through a different command
- "Re-benchmark encryption strength" raises an existing vault's key-derivation cost to whatever the current machine can afford, which a master-password change cannot do on its own (that only lifts a vault to the floor); it regenerates neither the vault key nor any record — only the 48-byte key wrapper is rewritten — so it is quick on a vault of any size, and it is clamped never to lower an existing cost, because calibration measures the machine as it is at that moment and a loaded host must not be able to weaken a strong vault
- The vault's recovery actions — restore a backup, remove the vault — now appear on the lock screen only once a lockout has begun, so "remove this vault" is not one careless click away for anyone who walks past the machine, while still remaining available during a lockout because a vault that cannot be unlocked must never be both unreachable and impossible to remove
- Password Vault auto-lock on idle timeout (default 5 minutes), on `Ctrl + Shift + L`, on the window being hidden, on application exit, and on resume from machine sleep, which is detected natively by comparing wall-clock against monotonic progress
- Password Vault isolation: the vault is not registered in the shared service container, so plugins and AI-facing surfaces cannot resolve it; only `vault.open` and `vault.lock` exist as commands, and neither can return a secret; the list projection omits passwords, notes, and custom field values, so a full item is fetched one at a time
- `crates/vault/tests/security.rs`, a threat-model test-suite asserting that no plaintext master password, username, password, note, title, URL, tag, or custom field ever appears in the vault file, its write-ahead log, or its sidecar files, alongside tamper detection, nonce uniqueness, lock cleanup, backup encryption, and key rotation
- `docs/vault-security.md`: the vault's threat model, key hierarchy, file format, isolation guarantees, and an explicit list of what it does not claim (no physical secure erase, no offline brute-force protection, no password recovery, no screenshot protection)
- `release.yml` workflow that bundles Linux, Windows, and macOS installers into a draft release on `v*` tags

### Security

- The vault master key is now pinned out of the system page file for as long as the vault is unlocked (`VirtualLock` on Windows, `mlock` on Unix), because wiping a key on drop does nothing about the copy the operating system may already have paged to disk; the lock is best-effort and its real status is reported to the UI rather than assumed, and the process's locked-page quota is raised on Windows, where the default allowed only eleven locked secrets
- Key derivation now has a hard memory floor of 64 MiB and 3 passes that calibration may raise but never lower; previously the weakest allowed configuration was also the starting point, so a slow machine — or a development build, where Argon2 runs unoptimised — persisted 19 MiB for the life of the vault, and changing the master password preserved that weakness
- Changing the master password now upgrades a vault whose key-derivation cost predates the current floor, and vault status reports whether a vault meets the floor, so the UI states the real strength instead of implying every vault is equally strong
- The master password itself, and the value a clipboard timer holds for up to a minute, are now pinned out of the page file alongside the vault keys; the master password is the root of the trust chain and is no longer merely wiped on drop, and locked buffers grow in whole pages so a long passphrase has no arbitrary length ceiling
- Restoring a backup now validates the header, the wrapped key, and every record *before* writing anything, so a corrupt or crafted backup is refused while the destination is still empty instead of producing a vault that can never be opened; a refused restore leaves the location usable and can simply be retried with the right file
- A fetched vault item now carries no secret at all: the password and every sensitive custom field are stripped natively before the item is serialised, so opening an item no longer puts a credential into the WebView, and a rendering bug cannot leak one that was never decrypted into it; a secret arrives only from an explicit per-field reveal, and copying a password does not return it to the caller at all because the native layer decrypts it, writes it to the clipboard, and schedules the clear itself
- Favouriting an item is now a native operation, and editing fetches the password only when the editor opens, so neither has to round-trip a redacted item back through the UI — which would otherwise have wiped the stored password
- Vault security settings now open as a modal from a button in the header instead of an inline panel at the bottom of the credential view; everything in it is vault-wide policy and the master password is typed into it, so it is deliberately kept away from the detail pane, and closing it — or the vault locking underneath it — wipes anything typed into it and moves focus back out

### Fixed

- SQLite now enables foreign keys per connection, so deleting a project cascades to its services
- Service names are trimmed and unique per project, and invalid definitions are rejected before they reach the database
- Log readers are aborted on stop instead of lingering on pipe handles inherited by surviving grandchild processes
- The desktop route now selects the project named in `/projects/:id`
- Replacing the application icon now actually changes the executable, window, and taskbar icon: `tauri-build` embeds `icons/icon.ico` without declaring it as a build input, so `build.rs` watches the icon directory and the Tauri configuration

### Release preparation

- `.github/` is no longer git-ignored. The CI workflow, the release workflow, the issue templates, and the Dependabot configuration were excluded from version control, so none of them would have existed on the public repository even though the README and `docs/development.md` linked to them
- The migration test now derives its expected list from the migration table itself instead of a hard-coded copy, which had gone stale when `0005_database_workbench` was added; `cargo test --workspace` had been failing on `main`
- The vault crate and the desktop backend are now `rustfmt`-clean and `clippy`-clean under `-D warnings`; `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets -- -D warnings` had both been failing, which is exactly what CI runs
- `LICENSE` no longer carries the Apache placeholder `[yyyy] [name of copyright owner]`
- The database preview fixture no longer publishes the maintainer's machine layout or an unrelated private project name, and its demo rows use placeholder identities
- The comment on the vault create screen no longer claims restore is reachable *only* there; it is available from the create screen, the lock screen, and the settings dialog, and the create screen is the one path that refuses to replace an existing vault
- Both READMEs now carry the brand mark and a technical overview — what each technology choice is responsible for, how the dependency direction, the native command surface, process ownership, and the three storage locations actually work — and `docs/development.md` records which asset is the canonical logo and which copies must change with it
- The vault backup section of `docs/vault-security.md` now describes the replacement behaviour that ships (settings dialog and lock screen replace, with a safety copy; the create screen refuses) instead of the earlier create-screen-only design, and its test count matches the suite

## 0.1.0-alpha.1

- Initial Tauri and Vue desktop workbench architecture
- Project persistence and stack scanning
- Command, context, service-runtime, log-streaming, process, and port foundations
- Light/dark developer-tool shell and command palette

