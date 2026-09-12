# Contributing

Dev Workbench welcomes focused, reviewable contributions.

1. Open or reference an issue describing the user-facing outcome.
2. Keep package boundaries intact: Vue views call application services or commands, never Tauri commands directly.
3. Add behavior tests for reusable logic and Rust tests for native behavior where practical.
4. Run `pnpm typecheck`, `pnpm test`, `pnpm build`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, and `cargo check --workspace`. The same checks run in CI on every pull request.
5. Do not include local paths, credentials, generated build output, or unrelated formatting changes.

Use clear commit messages and keep pull requests small enough to review. See [docs/development.md](docs/development.md) for prerequisites, repository layout, and working rules. By participating, you agree to follow the [Code of Conduct](CODE_OF_CONDUCT.md).

