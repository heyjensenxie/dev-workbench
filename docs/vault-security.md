# Password Vault — Security Design

> **Status: pre-release.** The vault is implemented and covered by an automated
> security test-suite, but it has **not** yet had the independent Security Review
> required before a public Beta (see [§11](#11-what-is-not-yet-done)). Until that
> review is complete, treat the vault as suitable for a single developer's own
> machine and nothing more.

## 1. What this is

The Password Vault stores a developer's own platform credentials — account,
password, login URL, notes, tags, and custom fields — entirely on the local
machine. It is not a feature of the workbench; it is a **separate security
domain** with its own encryption, its own storage, and a deny-by-default boundary
around it.

### The objective

No software running on a machine an attacker controls can promise "absolute
security", and this one does not. The objective is narrower, and it is testable:

> An attacker who obtains a copy of `vault.db` cannot recover any stored
> credential without knowing the master password.

Everything below exists to support that single statement.

### Non-goals for V1

Browser autofill, a browser extension, cloud sync, multi-user or team vaults, an
online account system, and TOTP codes. TOTP is planned for a later release and
will be encrypted under the same vault master key.

## 2. Key hierarchy

```
  master password
        │  Argon2id   (memory-hard KDF, per-machine cost, random 128-bit salt)
        ▼
  KEK  (key-encryption key — derived, never stored)
        │  XChaCha20-Poly1305
        ▼
  wrapped VMK  ──────► stored in the vault header (ciphertext only)
        │  XChaCha20-Poly1305, fresh random 192-bit nonce per record
        ▼
  encrypted item records ──► vault.db
```

The master password **is never used to encrypt data directly**. It derives a KEK,
and the KEK only ever wraps a randomly generated 256-bit **vault master key**
(VMK). That indirection is what makes a master-password change cheap and safe:
only the small wrapper around the VMK is rewritten, not the whole vault.

| Stage | Algorithm | Where it lives |
|---|---|---|
| Password → KEK | Argon2id (RFC 9106) | never stored |
| VMK | 256-bit random, OS CSPRNG | wrapped in the header |
| Record encryption | XChaCha20-Poly1305 | `vault_items` |

Nothing is hand-rolled. Every primitive comes from a maintained, widely reviewed
RustCrypto crate: `argon2` and `chacha20poly1305`. There is no custom
construction, no XOR scheme, no AES-ECB, no unauthenticated CBC, and no
"Base64 as encryption".

### Why XChaCha20-Poly1305

It provides confidentiality *and* integrity in one operation, and its 192-bit
nonce is what makes random nonces safe: the collision probability stays
negligible across any realistic number of encryptions under one key. Nothing is
keyed by a counter, a timestamp, or a record id. Every `seal` call draws a fresh
nonce from the OS CSPRNG.

### Why Argon2id

It is memory-hard, which is what makes GPU and ASIC cracking of a stolen vault
file expensive. The alternatives that must not be used — MD5, SHA-1, a bare
SHA-256 of the password, PBKDF2 — are not even reachable: the only KDF dependency
the crate declares is `argon2`.

Parameters are **calibrated against the machine that creates the vault**, with
memory cost doubled from a security floor until a single derivation would exceed
~500 ms, and parallelism following the available hardware threads. Measured on a
development machine in a release build, that lands at **256 MiB, 3 passes, 4
lanes, ~440 ms to unlock**. They are recorded in the header as plaintext, which is
safe and deliberate: KDF parameters are not secret, and recording them is what
allows the cost to be raised for a new vault or a password change without breaking
existing ones.

Calibration guarantees two things, and both matter more than hitting the target:

* **The result is never below the floor ([`MIN_MEMORY_KIB`] = 64 MiB, 3 passes).**
  The floor is measured first, so the returned configuration is never an
  unmeasured default.
* **A machine too slow to reach the target keeps the floor anyway.** Exceeding the
  target costs a slower unlock; dropping below the floor would cost the security
  property the vault exists to provide.

> **Why the floor matters, and why it was corrected.** The first implementation
> started from the weakest allowed configuration and only raised it after a
> successful measurement. On a slow machine — or a *debug* build, where Argon2
> runs unoptimised — the very first measurement exceeded the target, so the
> weakest possible parameters (19 MiB) were persisted for the lifetime of the
> vault. Since parameters live in the header, that vault stayed weak forever, and
> changing the master password preserved the weakness. Now the floor is enforced
> structurally.

### Raising the cost of an existing vault

Parameters are fixed when a vault is created, so a vault built by that earlier
version — or on a slower machine — stays weak until something rewrites them.
Rebuilding the application does **not** help: the parameters belong to the vault,
not to the program. Two paths fix it:

* **Changing the master password** lifts a vault to the floor. It is a deliberate
  action that already re-derives, so there is no extra cost, but it only ever
  reaches the floor.
* **Re-benchmarking** (`recalibrate`) runs the full machine calibration and takes
  whichever is stronger. This is the path to what the hardware can actually
  afford, and it is what the "Re-benchmark encryption strength" button in the
  settings dialog does.

Neither touches a record. The vault master key is not regenerated, so only the
48-byte wrapper around it is rewritten — the same property that makes a password
change cheap, and the reason re-benchmarking is quick on a vault of any size.
Re-benchmarking is clamped so it can **never lower** an existing cost: calibration
measures the machine as it is at that moment, and a temporarily loaded or
throttled host must not be able to talk a strong vault into weakening itself.

The salt is 128 bits from the OS CSPRNG and is regenerated on every master
password change.

## 3. Vault file format

`vault.db` is a SQLite database in the application data directory, **separate
from `workbench.sqlite3`**. That separation is not tidiness: the workspace
database is read and written by unrelated features, so a secret stored there
would sit inside the blast radius of every future query, export, migration, and
debugging tool.

```sql
CREATE TABLE vault_meta (
    id                INTEGER PRIMARY KEY CHECK (id = 1),
    format_version    INTEGER NOT NULL,
    kdf_algorithm     TEXT    NOT NULL,
    kdf_version       INTEGER NOT NULL,
    kdf_salt          BLOB    NOT NULL,
    kdf_memory_kib    INTEGER NOT NULL,
    kdf_time_cost     INTEGER NOT NULL,
    kdf_parallelism   INTEGER NOT NULL,
    kdf_output_len    INTEGER NOT NULL,
    wrapped_key_nonce BLOB    NOT NULL,
    wrapped_key_blob  BLOB    NOT NULL,
    created_at        INTEGER NOT NULL,
    updated_at        INTEGER NOT NULL
);

CREATE TABLE vault_items (
    id             TEXT    PRIMARY KEY NOT NULL,
    nonce          BLOB    NOT NULL,
    ciphertext     BLOB    NOT NULL,
    record_version INTEGER NOT NULL,
    created_at     INTEGER NOT NULL,
    updated_at     INTEGER NOT NULL
);
```

Read the schema and note what is **absent**: there is no `title`, `username`,
`password`, `url`, or `notes` column anywhere. The entire sensitive body of a
record — including the title, the login URL, and the tags — is serialised to JSON
and encrypted as one payload. Only an opaque random id, an opaque nonce, opaque
ciphertext, and non-secret timestamps are stored.

Titles and tags are treated as sensitive on purpose. Keeping them in plaintext
would make search convenient, and a plaintext index of "which services does this
developer have accounts on" is exactly the kind of metadata leak the vault
exists to prevent.

### Why this makes the surrounding storage concerns safe rather than lucky

Because **whatever crosses into SQLite is already ciphertext**:

* a write-ahead log can only ever contain ciphertext rows;
* temporary and shared-memory files can only ever contain ciphertext;
* `PRAGMA secure_delete=ON` is enabled as good hygiene, but it is *not* the
  security basis — see §9 for what is deliberately not claimed;
* file permissions are not relied upon either. An attacker who copies `vault.db`
  off the machine gets exactly what an attacker who reads it in place gets:
  ciphertext they cannot open.

### Associated data

Each record's ciphertext is bound to its own row id and to the format version via
AEAD associated data (`dev-workbench.vault.v1.item:<id>`). A ciphertext moved to
a different row — or replayed into a vault built by another format version —
fails authentication instead of silently decrypting. The wrapped VMK is bound the
same way to its own purpose string.

## 4. In-memory handling

* The VMK exists only in process memory, only while unlocked. It is never written
  to disk, never sent to the WebView, and never placed in the OS credential store.
* **Key material and the master password are pinned out of the page file.**
  Wiping a secret on drop only helps once it is gone; until then the operating
  system is free to page it to the swap file, which this application does not
  encrypt, and a master password sitting in a pagefile would compromise the vault
  as completely as a pagefile copy of the vault master key. `VaultKey`,
  `MasterPassword`, and the value a clipboard timer holds are therefore kept in
  page-locked memory (`VirtualLock` on Windows, `mlock` on Unix). Allocations are
  page-aligned and a whole number of pages, so a secret never shares a page with
  unrelated data — which matters, because locking rounds to page boundaries and
  the wipe step clears the *whole* allocation, including the padding.
* **The lock is reported, never assumed.** `VirtualLock`/`mlock` can be refused,
  so `VaultStatus.memoryLocked` carries the real answer and the UI states
  whichever is true. Two measured facts shaped this design:
  * Windows derives the lockable-page count from the process's *minimum* working
    set, about 50 pages by default. Measured on this project, that allowed only
    **11** locked 16 KiB allocations — far too few to rely on. `ensure_lock_quota`
    raises the working set once, and the same measurement then allowed all 300.
  * Because the quota is finite, each secret costs exactly the pages it needs and
    no more; the original allocation had been 4 pages per 32-byte key.
* Key material is held in types that wipe themselves on drop (`zeroize`,
  `secrecy`). `VaultKey` is not `Copy`; every clone is an independently wiped,
  independently locked allocation, so releasing the session lock before awaiting
  storage I/O does not leave an unmanaged copy behind.
* Nothing that holds secret bytes implements `Display`, and every `Debug`
  implementation is redacted. `format!("{:?}", item)` cannot leak a password into
  a log line or a panic message.
* Locking drops the key, which wipes it.
* **Returned data is the minimum the screen needs — and an item is redacted.** The
  list projection carries no password, notes, or custom field values, and a
  *fetched item* carries no password and no sensitive custom field either. What the
  detail pane renders is present; anything masked is not. `has_password` survives
  the redaction so the UI can offer a Reveal control without holding the value.
  Opening an item therefore does not put a credential into the renderer at all,
  and a rendering bug cannot leak one that was never decrypted into it.
* **A secret leaves only through one explicit, per-field request.** `reveal_field`
  returns a single value in locked memory; copying a password goes further and does
  not return it at all — the native layer decrypts it, writes it to the clipboard,
  and schedules the clear, so the plaintext never enters the WebView even for the
  most common action.
* Decrypted item payloads and the intermediate JSON produced before encryption are
  moved into `Zeroizing` buffers and wiped after use. These transient buffers are
  wiped but **not** page-locked: they live for milliseconds, whereas the VMK and
  the master password are the values that persist and are the ones pinned.

## 5. Locking

Locking is triggered by more than a timer, because a local vault is unlocked
while the user is at the machine, and every signal that the user may have left is
a reason to lock:

| Trigger | Mechanism |
|---|---|
| Idle timeout | Default 5 minutes; configurable 1 / 5 / 10 / 30. "Never" is not offered. |
| Manual lock | The Lock button and `Ctrl + Shift + L`, from anywhere in the app. No confirmation is required to lock. |
| Machine sleep | Detected natively: wall-clock progress is compared against monotonic progress. A monotonic clock does not advance while a machine sleeps, so an idle timer alone would let a closed laptop come back unlocked. |
| Application hidden | The window has been in the background for 60 seconds. |
| Application exit | Locks and clears the clipboard on `ExitRequested`. |
| Vault lock event | The native supervisor emits `vault-locked`; every open window drops its decrypted state. |

The idle deadline is enforced **natively** — in the Rust session, by a background
supervisor — not in the UI. A frozen, busy, or closed WebView cannot keep a vault
unlocked past its deadline, and the deadline is re-checked on every read so a
command can never operate on a key whose deadline has already passed.

### Unlock throttling

Distinct from locking, and much weaker by nature. Five consecutive failures lock
the vault out for 5 minutes; each further failure doubles that, capped at one
hour. A successful unlock is the only thing that resets the counter, so a lockout
that simply expires still escalates the next failure.

Three decisions worth stating:

* **The counter is persisted in the vault header**, not held in memory. Throttling
  that a restart clears is not throttling. It is the only mutable state outside the
  encrypted records, and it is not secret — a failure count and a timestamp.
* **The escalation is capped at one hour.** There is no master-password recovery,
  so an uncapped policy would let a curious colleague — or the owner fumbling a
  passphrase — make the vault permanently unreachable. Even at the ceiling this
  allows 24 guesses a day, which is worthless for guessing a real passphrase.
* **Every path that verifies the master password shares the throttle**: unlock,
  change-password, and key rotation. Otherwise the lockout would be side-stepped by
  guessing through a different command. The lockout is also checked *before* the
  key derivation, so a throttled caller cannot keep the CPU busy.

The recovery actions — restore a backup, remove the vault — appear only once the
lockout has actually begun. On a fresh lock screen they would put "remove this
vault" one careless click away for anyone who walked past the machine. They remain
available *during* a lockout, because a vault that cannot be unlocked must never be
both unreachable and impossible to remove.

On lock, the UI clears: the item list, the open item, every revealed value, the
search query, the editor draft, the generated password, and the clipboard
countdown. Leaving the screen hides revealed values, so returning to it cannot
show something that was on screen before.

## 6. Isolation: the deny-by-default boundary

```
   Projects  Services  API  Database  MCP  Plugins  AI
                     │
                     │  DENY BY DEFAULT
                     ✕
                     ▼
        ┌───────────────────────────────┐
        │        Password Vault         │
        │  master password → Argon2id   │
        │  → KEK → VMK → AEAD records   │
        └───────────────────────────────┘
```

| Surface | Guarantee |
|---|---|
| **Plugins** | The vault is *not* registered in the shared service container. That container is what every command — and therefore every plugin — receives, so leaving the vault out of it means `context.services.get('vault')` throws. A plugin cannot reach the vault even if it declares a `secrets` permission. |
| **AI** | Vault data is never placed in `WorkbenchContext`, any prompt-building structure, or any provider call. The module exposes no AI action of any kind. |
| **Commands** | Exactly two vault commands exist: `vault.open` (returns a navigation token, not data) and `vault.lock` (removes access). There is deliberately no `vault.getPassword`, `vault.listSecrets`, or `vault.exportPlaintext`, because the command registry is reachable from the palette, from plugins, and eventually from AI actions. || **Logs** | The vault logs nothing about its contents. Errors are coarse on purpose; a wrong password, a corrupted header, and a failed authentication tag all surface as the single message `Incorrect master password.` |
| **WebView** | The master password goes to Rust through a command and is never echoed back. A fetched item carries **no secret at all**: the password and every sensitive custom field are stripped natively before serialisation, so merely opening an item does not put a credential into the renderer. A secret arrives only from an explicit reveal, one field at a time — and copying a password does not put it in the WebView even then, because the native layer decrypts it, writes it to the clipboard, and never returns it. |
| **Clipboard** | See §7. |
| **Backups** | Encrypted only. See §8. |

`SecretStore` (the OS keychain boundary used for database passwords and API keys)
and the Password Vault are **separate security domains**. They share no code path
and no permission model. A plugin-visible secret capability must not be treated
as vault access.

## 7. Clipboard

Copying a password necessarily hands it to a system-wide resource that other
applications can read. That is an operating-system limitation, and the UI states
it plainly rather than implying the clipboard is private.

What the vault does control is the exposure window:

* a copied password is cleared after 15 / 30 / 60 seconds (default 30);
* the clear fires **only if the clipboard still holds exactly that value**, so it
  never destroys something the user copied in the meantime;
* a newer copy supersedes any pending timer for an older one;
* locking clears the clipboard immediately;
* usernames are copied without a scheduled clear, since they are not secrets in
  the same sense.

The clear is driven with a cancellable timer in the native layer, and the value
used for the comparison is held in a self-wiping buffer. On platforms without a
native implementation, the command fails loudly and the UI says that automatic
clearing is unavailable, rather than quietly doing nothing.

## 8. Backup

Exports are **always encrypted**. A `.vaultbackup` file contains the KDF
parameters, the wrapped VMK, and every record exactly as stored — still
ciphertext. There is no plaintext export path anywhere in the module.

There is deliberately no CSV or JSON export. If one is ever added it would need
re-authentication, an explicit warning, and a second confirmation; V1 does not
have one, and the destination path is normalised to the `.vaultbackup` extension
so a plaintext-friendly extension cannot be chosen by accident.

Import **replaces** an existing vault, but only from the surfaces where a user with
a vault actually is: the settings dialog and the lock screen. Both pass
`replaceExisting = true`, and before anything is written the native layer stores a
timestamped safety copy of the current vault and returns its path, so choosing the
wrong file is recoverable rather than final. The create screen is the exception —
it exists precisely because no vault is here, so it passes `replaceExisting = false`
and refuses rather than replaces if one appeared in the meantime.

The create screen is no longer the *only* place restore can be reached. It was
originally, because the rule was "import only when no vault exists" — and that rule
made the feature invisible to exactly the people who need it, since a user who
already has a vault never sees the create screen. A restored vault is ciphertext, so
it arrives **locked** and asks for the master password that protected that backup.

Import also **validates before writing anything**: an unsupported KDF, a salt that
is too short, a wrapped key that is not exactly a key plus an authentication tag,
or a record whose nonce is not 192 bits is rejected while the destination is still
empty — and, when replacing, before the existing vault is touched. Without that
check, a corrupt or crafted backup would be written successfully and produce a vault
that can never be opened, discovered at the moment the user is trying to recover
from a disaster. A refused import leaves the location usable, so the right file can
simply be chosen next.

## 9. What is deliberately not claimed

Honesty about limits is part of the design. The vault does **not** claim:

* **Physical erasure.** Deleting an item removes a row, and removing the vault
  deletes every row, checkpoints the write-ahead log, and rebuilds the file. That
  removes the vault from everything SQLite can reach — but SQLite, the filesystem
  journal, and above all SSD wear-levelling and over-provisioning all keep copies
  the application cannot reach. There is no "secure erase" here, and both the
  delete dialog and the remove-vault dialog say so.
* **Password-gating the removal of a local vault.** "Remove local vault" is
  deliberately unauthenticated. Deleting a local file never required the master
  password — anyone with access to the machine can remove `vault.db` directly — so
  a prompt would be theatre that also blocks the case that needs the operation
  most: a vault that cannot be unlocked, such as a backup restored with a password
  its owner no longer has. The guard is deliberate friction instead — the user must
  type a confirmation word — and it is described as friction, not as a security
  control.
* **Offline brute-force prevention.** An attacker with a copy of the file never
  runs this application, so nothing here — including the unlock lockout — is in
  their path. The lockout is an **interactive** throttle: it stops someone sitting
  at an unlocked machine from working through a list of guesses against the unlock
  prompt. Its counter lives in the vault header in plaintext, and an attacker with
  write access can simply reset it. The real defences against a stolen file are a
  strong master password and a costly Argon2id derivation.
* **Privacy from the operating system.** Malware running with the user's
  privileges, a keylogger, cold-boot or DMA attacks on live memory, and a
  compromised OS are all out of scope.
* **Password recovery.** There is no server, no account, no recovery key, and no
  backdoor. Losing the master password means losing the vault. The creation
  screen requires an explicit acknowledgement of that before a vault can be
  created; there is no security question and no recovery hint.
* **Screenshot protection.** Screen-capture prevention is a platform-specific,
  unreliable capability. It is not implemented and not advertised; it is a
  candidate for a future hardening pass. What *is* implemented is that a revealed
  password re-hides itself on a timer, when the item changes, and when the screen
  is left.
* **Quick Unlock.** Windows Hello / Touch ID / Secure Enclave unlock is not in
  V1. If it is added it must be opt-in, clearly explained as a
  security-versus-convenience trade-off, and must never place the raw VMK in an
  ordinary credential store.
* **Protection from the machine's own operator.** An unlocked vault on an
  unattended machine is readable by anything running as that user. Page-file
  protection covers the *idle* case — a locked screen, a suspended process — but
  it does not stop malware, a debugger, or a memory dump from reading live
  process memory. Locking the vault is what ends the exposure.
* **A locked-page quota on Unix.** `mlock` is bounded by `RLIMIT_MEMLOCK`, which a
  library has no business changing for its host process. Where the limit is too
  low for even one page, `memoryLocked` reports `false` and the UI says the secret
  is protected only by being wiped on lock.

## 10. Automated security tests

`crates/vault/tests/security.rs` is written against the threat model rather than
the implementation, so a refactor that weakens a guarantee fails a test whose
intent is obvious from its name.

| # | Requirement | Test |
|---|---|---|
| 1 | File contains no master password | `vault_file_never_contains_the_master_password` |
| 2 | File contains no username | `vault_file_never_contains_the_username` |
| 3 | File contains no password | `vault_file_never_contains_the_password` |
| 4 | File contains no notes | `vault_file_never_contains_the_notes` |
| — | No plaintext title, URL, tag, or custom field | `vault_file_never_contains_even_the_title_url_tag_or_custom_field` |
| — | No plaintext in WAL or sidecar files | `the_vault_never_writes_plaintext_to_a_sidecar_file` |
| 5 | Correct password decrypts | `the_correct_master_password_decrypts` |
| 6 | Wrong password cannot decrypt | `a_wrong_master_password_cannot_decrypt`, `the_wrong_password_error_leaks_no_cryptographic_detail` |
| 7 | Modified ciphertext / nonce / tag cannot decrypt | `a_modified_ciphertext_cannot_be_decrypted`, `a_modified_nonce_cannot_be_decrypted`, `a_modified_authentication_tag_cannot_be_decrypted` |
| 8 | Nonces never repeat | `identical_plaintext_never_produces_a_repeated_nonce_or_ciphertext` (+ unit test over 2 000 seals) |
| 9 | Old password fails, new password works | `changing_the_master_password_invalidates_the_old_one`, `changing_the_master_password_does_not_re_encrypt_records` |
| 10 | Lock clears in-memory state | `locking_clears_the_in_memory_key_and_refuses_reads`, `the_idle_timeout_locks_the_vault_and_drops_its_key` |
| 11 | Sensitive fields never reach logs | Redacted-`Debug` unit tests in `model.rs` and `key.rs`; coarse-error tests |
| 12 | No plaintext in any persistent store | Requirements 1–4 plus the sidecar test |
| 13 | Password never enters command history | `packages/core/src/vault-isolation.test.ts` |
| 14 | Password never enters `WorkbenchContext` | `packages/core/src/vault-isolation.test.ts` |
| 15 | Plugins cannot reach the vault | `packages/core/src/vault-isolation.test.ts` |
| 16 | AI cannot reach the vault | `packages/core/src/vault-isolation.test.ts` |
| — | UI clears state on every lock path | `apps/desktop/src/stores/vault.test.ts` |
| — | URL scheme allow-list (UI and native) | `packages/shared/src/vault.test.ts`, `vault.rs` unit tests |

Additional coverage: AEAD round-trip and tamper detection, generator output
properties, ciphertext-relocation rejection, backup encryption and restore,
key rotation, and a guard asserting that the production entry point never picks
the cheap test-only KDF parameters.

Run them with:

```bash
cargo test -p workbench-vault     # 144 tests: crypto, KDF, locked memory, session, and the security suite
cargo test -p dev-workbench       # native command-layer unit tests
pnpm test                         # shared, core, and UI-store tests
```

These exist specifically to stop the guarantees above from eroding:

| Guard | Test |
|---|---|
| Calibration cannot return a below-floor configuration, however cheap the target | `calibration_never_goes_below_the_floor_however_cheap_the_target` (uses a zero-duration target) |
| A legacy vault keeps opening, and is reported as below the floor rather than silently treated as strong | `a_vault_below_the_floor_is_reported_but_still_opens` |
| Changing the master password repairs a below-floor vault | `changing_the_master_password_repairs_a_below_floor_vault`, `recalibrating_lifts_a_legacy_vault_above_the_floor` |
| Re-benchmarking strengthens a vault without re-encrypting a single record, and can never weaken one | `recalibrating_strengthens_the_parameters_without_touching_a_record`, `recalibrating_never_weakens_an_existing_configuration`, `a_recalibrated_vault_still_opens_with_the_same_password_and_data` |
| The vault master key, the master password, and every key clone really are pinned | `the_vault_master_key_is_held_in_locked_memory`, `the_master_password_is_held_in_locked_memory`, `a_key_is_held_in_locked_memory` |
| The locked-page quota was actually raised | `many_locked_values_can_coexist` (256 keys) |
| Secrets never share a page with unrelated data, and the wipe clears the whole allocation | `the_allocation_is_page_complete`, `wiping_clears_the_entire_allocation`, `a_multi_page_secret_is_fully_wiped` |
| A long secret is accommodated with no arbitrary length ceiling | `a_secret_far_larger_than_one_page_is_accommodated` |
| A malformed restore is refused *before* it can create an unopenable vault, and can be retried | `importing_a_backup_with_an_unsupported_kdf_is_refused`, `importing_a_backup_with_a_truncated_wrapped_key_is_refused`, `importing_a_backup_with_an_unusable_record_is_refused`, `a_refused_import_can_be_retried_with_a_good_backup` |
| Opening an item never brings a secret into the UI process | `a_fetched_item_never_carries_a_secret_value`, `never holds a password just because an item was opened` |
| A secret is reachable only one field at a time, for the named item | `reveal_only_yields_sensitive_fields_and_only_for_the_named_item`, `produces a secret only from an explicit per-field reveal` |
| Copying a password does not return its plaintext to the caller | `copies a secret natively without returning it to the caller`, `copies a password natively and never receives its plaintext` |
| Favouriting and editing cannot silently destroy a stored password | `favouriting_an_item_does_not_require_its_password`, `updating_an_item_without_a_password_clears_it_deliberately` |
| A reveal that resolves after the user moved on is discarded | `ignores a reveal that arrives after the user moved to another item` |
| Removing the vault really removes it, key included, and leaves no ciphertext in the write-ahead log | `destroying_the_vault_removes_every_credential_and_the_key`, `destroying_leaves_the_write_ahead_log_free_of_ciphertext` |
| A removed vault can be replaced by a new one or by a different backup — the escape hatch actually escapes | `a_destroyed_vault_can_be_replaced_by_a_different_one`, `a_destroyed_vault_can_be_replaced_by_a_restored_backup`, `unblocks the restore path afterwards` |
| Below the threshold, a failure stays indistinguishable from any other wrong password | `early_failures_are_answered_with_the_uniform_password_error`, `a_locked_out_vault_refuses_even_the_correct_password` |
| The lockout escalates, persists across a restart, and cannot exceed its ceiling | `a_failure_after_an_expired_lockout_escalates`, `the_lockout_survives_restarting_the_application`, `the_lockout_never_exceeds_its_ceiling` |
| Guessing cannot be redirected to a different command, and the way out stays open | `changing_the_master_password_shares_the_throttle`, `a_locked_out_vault_refuses_password_guessing_through_every_path`, `the_escape_hatch_still_works_while_locked_out` |
| The UI offers the recovery actions only once the lockout has begun | `hides the recovery actions until the vault is locked out`, `counts the lockout down and keeps the underlying attempts after it lapses` |

## 11. What is not yet done

Before this module may enter a public Beta it requires an **independent security
review** covering: the cryptography and key lifecycle, secret logging, clipboard
handling, the Tauri IPC surface, WebView state, the SQLite layer, backup and
migration, and plugin and AI isolation. Passing tests are necessary but not
sufficient.

Known follow-ups, in rough priority order:

1. Independent security review (blocking for Beta).
2. Vault key rotation is implemented and tested, but has no UI. It is reachable
   only from the test-suite; the format reserves it.
3. TOTP support (V1.1), encrypted under the same VMK.
4. Screen-capture protection, if and only if it can be implemented reliably per
   platform — otherwise it must not be advertised.
5. Optional Quick Unlock through the OS secure enclave, opt-in and clearly
   explained.
6. Page-locking the transient plaintext buffers (decrypted item payloads, the
   intermediate JSON before encryption) as well as the persistent secrets.

## 12. Module layout

```
crates/vault/
├── src/
│   ├── crypto.rs     AEAD boundary — the only place confidentiality is produced
│   ├── kdf.rs        Argon2id parameters, calibration, derivation
│   ├── key.rs        VaultKey / MasterPassword / VMK wrapping
│   ├── model.rs      encrypted payload + list projection + redacted Debug
│   ├── storage.rs    ciphertext-only SQLite schema and transactions
│   ├── session.rs    in-memory unlock state, idle timeout, sleep detection
│   ├── generator.rs  password generation from the OS CSPRNG
│   ├── clipboard.rs  native clipboard with guarded auto-clear
│   ├── service.rs    the only supported way to touch vault data
│   └── error.rs      deliberately coarse error surface
└── tests/security.rs threat-model test-suite

apps/desktop/src-tauri/src/vault.rs   Tauri command surface (thin, no crypto)
apps/desktop/src/stores/vault.ts      UI state; cleared on every lock
apps/desktop/src/views/VaultView.vue  lock screen, list, detail, editor, settings
packages/shared/src/vault.ts          types, URL allow-list, strength, search
packages/core/src/index.ts            VaultCatalog + the two permitted commands
```

`VaultService` is the front door. Callers never receive a `VaultKey`, a KEK, a
raw record, or a database handle, and they never choose a nonce or associated
data. Any future capability that wants to read a vault secret must go through a
new security design; being inside the same application grants nothing.
