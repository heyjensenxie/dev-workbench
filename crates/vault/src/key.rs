//! Key material wrappers.
//!
//! Three rules hold throughout this module:
//!
//! 1. Nothing that holds secret bytes implements `Display`, and every `Debug`
//!    implementation is redacted. A `format!("{:?}", item)` must never be able
//!    to leak a password into a log line or a panic message.
//! 2. Every owned copy of key material is wiped on drop. `VaultKey` is
//!    deliberately *not* `Copy`, and cloning is explicit so that each clone is
//!    itself an independently wiped allocation.
//! 3. Key bytes live in [`LockedBytes`], which asks the operating system to keep
//!    them out of the pagefile for as long as they exist. Wiping a key only
//!    helps once it is gone; until then it must not be able to reach the disk.

use std::fmt;

use rand::RngCore;
use rand::rngs::OsRng;
use zeroize::Zeroize;

use crate::crypto;
use crate::error::{VaultError, VaultResult};
use crate::locked::{LockedBuffer, LockedBytes};

/// Symmetric key length in bytes (256-bit).
pub const KEY_LEN: usize = 32;

/// Associated data binding a wrapped vault master key to its purpose and format
/// version. Changing the format requires a new constant.
const VMK_AAD: &[u8] = b"dev-workbench.vault.v1.vault-master-key";

/// Minimum accepted master password length. Length is favoured over character
/// class rules on purpose: a long passphrase beats a short "complex" password.
pub const MASTER_PASSWORD_MIN_LEN: usize = 12;

/// A 256-bit symmetric key (either a KEK or the vault master key).
///
/// The bytes are held in locked, non-pageable memory and overwritten on drop.
/// There is intentionally no `Display`, no `Serialize`, and no `PartialEq` that
/// could be used as an oracle; comparisons that matter are done by the AEAD tag.
pub struct VaultKey(LockedBytes<KEY_LEN>);

impl VaultKey {
    /// Draws a fresh key from the operating system CSPRNG.
    pub fn generate() -> Self {
        let mut bytes = [0u8; KEY_LEN];
        OsRng.fill_bytes(&mut bytes);
        Self::from_bytes(bytes)
    }

    /// Adopts raw key bytes, taking ownership. The source buffer is wiped.
    pub fn from_bytes(mut bytes: [u8; KEY_LEN]) -> Self {
        let key = Self(LockedBytes::new(bytes));
        // The caller's stack copy is a second plaintext copy; clear it.
        bytes.zeroize();
        key
    }

    /// Borrows the raw key bytes. The borrow is short-lived by construction.
    pub(crate) fn expose(&self) -> &[u8; KEY_LEN] {
        self.0.as_bytes()
    }

    /// Whether this key's memory was successfully pinned out of the pagefile.
    ///
    /// Reported rather than assumed: on a platform without an implementation, or
    /// when the operating system refuses the request, this is `false` and the key
    /// is protected only by being wiped on drop.
    pub fn is_memory_locked(&self) -> bool {
        self.0.is_locked()
    }
}

impl Clone for VaultKey {
    /// Produces an independent, independently-wiped, independently-locked copy.
    /// Used to release the session lock before awaiting storage I/O.
    fn clone(&self) -> Self {
        Self(LockedBytes::new(*self.0.as_bytes()))
    }
}

impl fmt::Debug for VaultKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("VaultKey(<redacted>)")
    }
}

/// A vault master key wrapped (encrypted) under a key derived from the master
/// password. This is the only representation of the VMK that may be persisted.
#[derive(Clone)]
pub struct WrappedKey {
    pub nonce: Vec<u8>,
    pub ciphertext: Vec<u8>,
}

impl fmt::Debug for WrappedKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("WrappedKey(<redacted>)")
    }
}

/// Encrypts the vault master key so it can be stored at rest.
pub fn wrap_key(kek: &VaultKey, vmk: &VaultKey) -> VaultResult<WrappedKey> {
    let (nonce, ciphertext) = crypto::seal(kek, VMK_AAD, vmk.expose())?;
    Ok(WrappedKey { nonce, ciphertext })
}

/// Recovers the vault master key. Any authentication failure is reported as a
/// tamper error; callers collapse that into the uniform password error.
pub fn unwrap_key(kek: &VaultKey, wrapped: &WrappedKey) -> VaultResult<VaultKey> {
    let plaintext = crypto::open(kek, VMK_AAD, &wrapped.nonce, &wrapped.ciphertext)?;
    let bytes: [u8; KEY_LEN] = plaintext
        .as_slice()
        .try_into()
        .map_err(|_| VaultError::Corrupt("wrapped key length"))?;
    Ok(VaultKey::from_bytes(bytes))
}

/// The user's master password.
///
/// Held only for the duration of a single create/unlock/change-password call. It
/// is never persisted, never logged, never sent to the WebView, and never placed
/// on the clipboard.
///
/// It lives in [`LockedBuffer`] rather than an ordinary heap string, for the same
/// reason the keys do: this is the root of the entire trust chain, and a pagefile
/// copy of it would compromise the vault just as completely as a pagefile copy of
/// the vault master key. The source `String` it is built from is wiped.
///
/// The type has no `Display` and a redacted `Debug`.
pub struct MasterPassword(LockedBuffer);

impl MasterPassword {
    /// Adopts a password received as an owned `String`, wiping the source buffer.
    pub fn new(value: String) -> Self {
        Self(LockedBuffer::take_string(value))
    }

    /// Borrows the password as a string slice for the KDF call.
    pub fn expose(&self) -> &str {
        self.0.as_str()
    }

    /// Whether the password's memory was pinned out of the page file.
    pub fn is_memory_locked(&self) -> bool {
        self.0.is_locked()
    }

    /// Rejects passwords that are too short to be worth deriving against.
    /// Deliberately does not demand character classes.
    pub fn validate(&self) -> VaultResult<()> {
        if self.expose().chars().count() < MASTER_PASSWORD_MIN_LEN {
            return Err(VaultError::Validation(format!(
                "master password must be at least {MASTER_PASSWORD_MIN_LEN} characters"
            )));
        }
        Ok(())
    }
}

impl fmt::Debug for MasterPassword {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("MasterPassword(<redacted>)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_never_reveals_key_or_password() {
        let key = VaultKey::from_bytes([7u8; KEY_LEN]);
        assert_eq!(format!("{key:?}"), "VaultKey(<redacted>)");
        let password = MasterPassword::new("correct-horse-battery".into());
        assert!(!format!("{password:?}").contains("horse"));
    }

    #[test]
    fn the_master_password_is_held_in_locked_memory() {
        if !crate::locked::is_supported() {
            return;
        }
        let password = MasterPassword::new("correct-horse-battery-staple".into());
        assert!(
            password.is_memory_locked(),
            "the root secret of the vault must be pinned out of the page file"
        );
    }

    #[test]
    fn a_key_is_held_in_locked_memory() {
        if !crate::locked::is_supported() {
            return;
        }
        assert!(VaultKey::generate().is_memory_locked());
        // Every clone must be pinned as well; a clone is used for each operation
        // that releases the session lock.
        assert!(VaultKey::generate().clone().is_memory_locked());
    }

    #[test]
    fn wrapped_key_round_trips_under_the_same_kek() {
        let kek = VaultKey::generate();
        let vmk = VaultKey::generate();
        let wrapped = wrap_key(&kek, &vmk).expect("wrap");
        let recovered = unwrap_key(&kek, &wrapped).expect("unwrap");
        assert_eq!(recovered.expose(), vmk.expose());
    }

    #[test]
    fn a_different_kek_cannot_unwrap() {
        let vmk = VaultKey::generate();
        let wrapped = wrap_key(&VaultKey::generate(), &vmk).expect("wrap");
        assert!(unwrap_key(&VaultKey::generate(), &wrapped).is_err());
    }

    #[test]
    fn a_tampered_wrapped_key_is_rejected() {
        let kek = VaultKey::generate();
        let mut wrapped = wrap_key(&kek, &VaultKey::generate()).expect("wrap");
        wrapped.ciphertext[0] ^= 0x01;
        assert!(unwrap_key(&kek, &wrapped).is_err());
    }

    #[test]
    fn a_flipped_wrapped_key_bit_flips_a_tag_failure() {
        let kek = VaultKey::generate();
        let mut wrapped = wrap_key(&kek, &VaultKey::generate()).expect("wrap");
        let last = wrapped.ciphertext.len() - 1;
        wrapped.ciphertext[last] ^= 0x80;
        assert!(matches!(
            unwrap_key(&kek, &wrapped),
            Err(VaultError::Tampered)
        ));
    }

    #[test]
    fn short_master_passwords_are_rejected_without_character_class_rules() {
        assert!(MasterPassword::new("short".into()).validate().is_err());
        assert!(
            MasterPassword::new("elevenchars".into()).validate().is_err(),
            "11 characters is still below the floor"
        );
        assert!(
            MasterPassword::new("twelvecharss".into()).validate().is_ok(),
            "12 lowercase characters is acceptable"
        );
    }
}
