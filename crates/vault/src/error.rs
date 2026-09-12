//! Error surface for the vault boundary.
//!
//! Every `Display` implementation here is deliberately coarse. Vault errors
//! travel to the UI, into logs, and potentially into crash reports, so they must
//! never carry key material, derived keys, ciphertext, KDF parameters, or any
//! hint about *which* stage of decryption failed. Callers that need diagnostics
//! should inspect the `#[source]` chain locally instead of rendering it.

use thiserror::Error;

pub type VaultResult<T> = Result<T, VaultError>;

#[derive(Debug, Error)]
pub enum VaultError {
    #[error("vault storage is unavailable")]
    Storage(#[source] sqlx::Error),
    #[error("vault file access failed")]
    Io(#[from] std::io::Error),
    #[error("vault has not been created yet")]
    NotInitialized,
    #[error("a vault already exists in this location")]
    AlreadyInitialized,
    /// The single, uniform failure message for a wrong master password. It must
    /// not distinguish a bad password from a corrupted header or a failed
    /// authentication tag.
    #[error("Incorrect master password.")]
    IncorrectMasterPassword,
    /// Integrity failure: ciphertext, nonce, or associated data was modified.
    #[error("vault data failed its integrity check")]
    Tampered,
    #[error("vault is locked")]
    Locked,
    /// Too many consecutive failed unlock attempts.
    ///
    /// Carries the remaining time because the user has to be told when to come
    /// back. Unlike the wrong-password error there is nothing to conceal here:
    /// the lockout is visible in the vault header anyway.
    #[error("Too many failed attempts. Try again in {seconds} seconds.")]
    LockedOut { seconds: u64 },
    #[error("stored vault data is not readable ({0})")]
    Corrupt(&'static str),
    #[error("invalid input: {0}")]
    Validation(String),
    #[error("vault cryptography failed ({0})")]
    Crypto(&'static str),
    #[error("vault backup failed ({0})")]
    Backup(&'static str),
    #[error("the operating system does not support secure clipboard handling on this platform")]
    ClipboardUnsupported,
}
