//! # Dev Workbench Password Vault
//!
//! An isolated local credential store. This crate is a *security boundary*, not
//! a feature module: nothing else in Dev Workbench links against the vault's
//! internals, and the vault links against nothing else in Dev Workbench.
//!
//! ## Design objective
//!
//! No system that runs on a machine the attacker controls can promise "absolute
//! security", and this one does not. The objective is narrower and testable:
//!
//! > An attacker who obtains a copy of `vault.db` cannot recover any stored
//! > credential without knowing the master password.
//!
//! That is achieved with established, externally-audited cryptography and
//! nothing else. No algorithm here is invented, and every primitive comes from a
//! maintained RustCrypto crate.
//!
//! ## Key hierarchy
//!
//! ```text
//!   master password
//!         │  Argon2id (memory-hard KDF, per-machine calibrated, random salt)
//!         ▼
//!   KEK  (key-encryption key, never stored)
//!         │  XChaCha20-Poly1305
//!         ▼
//!   wrapped VMK  ── stored in the vault header
//!         │  XChaCha20-Poly1305, fresh 192-bit nonce per record
//!         ▼
//!   encrypted item records
//! ```
//!
//! The indirection through a randomly generated vault master key (VMK) is what
//! makes changing the master password cheap and safe: only the small wrapper
//! around the VMK is rewritten, not the whole vault. The master password is
//! never used directly as a data-encryption key.
//!
//! ## Threat model
//!
//! | Adversary capability | Outcome |
//! |---|---|
//! | Copies `vault.db` | Ciphertext only; no credential recoverable without the master password |
//! | Reads the workspace database (`workbench.sqlite3`) | Finds nothing; vault data never enters it |
//! | Reads application logs or stack traces | Finds nothing; secret types have redacted `Debug` and errors are deliberately coarse |
//! | Reads the AI context, telemetry, or crash reports | Finds nothing; no vault data is placed there |
//! | Reads the WebView / DevTools state | Finds only what is currently displayed; passwords are fetched one item at a time, never as a list |
//! | Reads plugin APIs | Finds nothing; no vault capability is exposed to plugins |
//! | Reads the command registry / command history | Finds nothing; only `vault.open` and `vault.lock` exist, and neither returns a secret |
//! | Inspects the system clipboard | *Can* read a copied password — an operating-system limitation the UI states plainly, bounded by an automatic clear |
//! | Walks up to an unlocked, unattended machine | Bounded by auto-lock on idle, on sleep/wake, and on application exit |
//! | Modifies the vault file | Detected; AEAD authentication fails and decryption is refused |
//!
//! Out of scope, and not claimed anywhere: malware already running with the
//! user's privileges, a keylogger, cold-boot or DMA attacks on live memory, a
//! compromised operating system, and physical erasure guarantees on SSDs.
//! Recovering a lost master password is also out of scope by design: there is no
//! server, no account, no recovery key, and no backdoor.

pub mod clipboard;
pub mod crypto;
pub mod error;
pub mod generator;
pub mod kdf;
pub mod key;
pub mod locked;
pub mod lockout;
pub mod model;
pub mod service;
pub mod session;
pub mod storage;

pub use error::{VaultError, VaultResult};
pub use generator::GeneratorOptions;
pub use key::MasterPassword;
pub use model::{
    VaultBackupSummary, VaultItem, VaultItemField, VaultItemPayload, VaultItemSummary, VaultStatus,
};
pub use service::VaultService;
pub use session::{AUTO_LOCK_OPTIONS_SECONDS, DEFAULT_AUTO_LOCK_SECONDS, VaultSession};

// Re-exported so host crates can handle vault secrets without adding their own
// dependency on these crates, and so both sides are guaranteed to use the same
// versions of the wiping types.
pub use secrecy;
pub use zeroize;

/// Current wall-clock time in milliseconds since the Unix epoch.
pub(crate) fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}
