# Security Policy

## Supported versions

Dev Workbench is pre-release software. Only the most recent `0.1.0-alpha.N` tag is
supported; there are no maintained older branches, and a report against a build
from an untagged commit will usually be answered with "please reproduce on the
latest tag".

Current release: `0.1.0-alpha.2` (2026-09-13).

This is an early development release and is not recommended for security-critical
workloads.

## Reporting a vulnerability

Report vulnerabilities privately through GitHub's
[security advisory form](https://github.com/heyjensenxie/dev-workbench/security/advisories/new)
rather than a public issue. Include reproduction steps, the affected platform and
version, and the impact where you can.

Please **do not** open a public issue, and do not include a working exploit or real
credentials in a report. There is no bug bounty and no response-time guarantee —
this is a small project — but every report is read, and a confirmed vulnerability
is fixed before it is described publicly.

## Security boundaries

The project's security-relevant behaviour is not spread evenly across the code.
These are the boundaries a report should be judged against.

### The Password Vault

`crates/vault` stores credentials and is the one component whose security claims
are written down in full. [docs/vault-security.md](docs/vault-security.md) is the
authoritative document: it defines the threat model, the key hierarchy, the file
format, how secrets are kept out of the WebView and the logs, and — importantly —
an explicit list of what the vault does **not** claim.

Read it before reporting a vault issue, because several things that look like
vulnerabilities are stated non-goals, including:

- no protection against malware, a debugger, or a memory dump on an unlocked vault;
- no offline brute-force protection — an attacker with a copy of `vault.db` never
  runs this application, so the unlock lockout is not in their path;
- no physical secure erase, no password recovery, and no screenshot protection;
- "remove local vault" is deliberately unauthenticated, because deleting a local
  file never required the master password.

The vault has been implemented with an automated threat-model test suite, but it
has **not** had the independent security review that
[docs/vault-security.md §11](docs/vault-security.md#11-what-is-not-yet-done)
requires before a public Beta. A finding in the cryptography, the key lifecycle,
the clipboard handling, the Tauri IPC surface, or the vault's isolation from
plugins and AI surfaces is in scope and welcome.

### Secrets and storage

- Nothing secret may be persisted in the workspace SQLite database. The schema has
  no column for a password, token, or key, and it must not grow one.
- Database connection passwords go to the operating system credential store
  through `crates/secrets`, and the connection row keeps only a `secretRef`
  marker. Note that this boundary is implemented on Windows only today; on Linux
  and macOS the operation fails explicitly rather than degrading to plaintext.
- API request templates are resolved from session-only environment variables at
  send time, so a saved request must never contain a resolved secret. Request
  history keeps a redacted URL.
- `SecretStore` and the Password Vault are **separate security domains**. They
  share no code path and no permission model; a plugin-visible secret capability
  must not be treated as vault access.

### Process execution and logs

- The application starts processes on the user's behalf, so treat a command line
  and its environment as sensitive: they must not be written to diagnostic logs
  without redaction.
- Service commands come from the user's own project manifests. The scanner reads
  well-known manifest files only, and must not execute anything it finds.
- Environment variables are persisted per service. They are not encrypted, which
  is deliberate and is why the UI does not present them as a place for secrets.

### Network

- The only outbound request the application makes is one the user explicitly
  sends from the API Workbench, through the native `reqwest` client. There is no
  telemetry, no update check, and no account.
- Production builds restrict the WebView with a Content Security Policy: scripts
  and styles are limited to bundled assets, connections are allowed only to the
  Tauri IPC and self, and objects, framing, and form submission are forbidden.
  A relaxation of that policy is a security change and should be reported as one.

### Dependencies

Dependency updates arrive through Dependabot (`.github/dependabot.yml`). If you
find a vulnerable dependency, report it through the advisory form and, where the
upgrade is not mechanical, say what you believe the fix is.
