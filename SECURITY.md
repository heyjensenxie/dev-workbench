# Security Policy

## Supported versions

`0.1.0-alpha.1` is an early development release and is not recommended for security-critical workloads.

## Reporting

Please report vulnerabilities privately through GitHub's security advisory feature rather than a public issue. Include reproduction steps, affected platforms, and impact where possible.

Never store passwords, tokens, private keys, or API keys in the SQLite database. Future secret-bearing features must use the operating system keychain through the native secrets boundary. Process commands and environment values should be treated as sensitive and must not be written to diagnostic logs without redaction.

