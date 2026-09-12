<!--
Thanks for contributing. Keep the pull request small enough to review, and read
CONTRIBUTING.md and docs/development.md before opening it.
-->

## What this changes

<!-- The user-visible outcome, not a file list. Link the issue it closes. -->

Closes #

## How it was verified

<!--
Name the checks you ran and the manual steps you took. Do not tick a box for a
command you did not actually run — CI will tell us anyway, and the honest answer
is more useful than the tidy one.
-->

- [ ] `pnpm typecheck`
- [ ] `pnpm test`
- [ ] `pnpm build`
- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`

## Checklist

- [ ] The dependency direction is intact: a view calls an application service or a command, and only `apps/desktop/src/services/nativeBridge.ts` calls `invoke`.
- [ ] Cross-boundary models live in `packages/shared`, and validation happens both there (for UI feedback) and on the Rust side (so a caller that skips the UI cannot corrupt the database).
- [ ] No credential, token, API key, password, private key, or local absolute path is added to any file — including fixtures, test data, and comments.
- [ ] Nothing secret is written to the SQLite database. Secrets go to `crates/secrets`; the vault stays outside the shared service container.
- [ ] `crates/{docker,git,pty}` and `plugins/*` are still described as ownership boundaries rather than implemented features.
- [ ] Documentation that the change invalidates has been updated (`README.md`, `README.en.md`, `docs/`, `CHANGELOG.md`).
- [ ] The change is covered by a test where the behaviour is testable, and no existing test was weakened to make it pass.

## Notes for the reviewer

<!--
Anything that needs context: a trade-off you made, a rule you deliberately did not
follow, or a follow-up you are leaving out of scope.
-->
