//! Authenticated encryption boundary.
//!
//! This module is the *only* place in Dev Workbench where confidentiality of
//! vault data is produced. It wraps `chacha20poly1305`, the RustCrypto
//! implementation of XChaCha20-Poly1305 (an IETF-standardised AEAD built on
//! ChaCha20 and Poly1305).
//!
//! Properties relied upon elsewhere in the crate:
//!
//! * **Confidentiality and integrity together.** A modified ciphertext, nonce,
//!   or associated data makes `open` fail; there is no code path that returns
//!   unauthenticated plaintext.
//! * **Unique nonces.** Every `seal` draws a fresh 192-bit nonce from the OS
//!   CSPRNG. 192 bits of randomness is what makes random nonces safe here: the
//!   probability of a collision stays negligible across any realistic number of
//!   encryptions under one key. Nothing is ever keyed by a counter, a
//!   timestamp, or a record id.
//! * **Bound context.** Callers pass associated data (AAD) that binds a
//!   ciphertext to its purpose and to the record it belongs to, so a ciphertext
//!   cannot be relocated into another row or another vault.
//!
//! Nothing here is hand-rolled: no XOR, no custom construction, no key
//! derivation, no padding scheme.

use chacha20poly1305::{
    Key, Tag, XChaCha20Poly1305, XNonce,
    aead::{AeadCore, AeadInPlace, KeyInit},
};
use rand::rngs::OsRng;
use zeroize::Zeroizing;

use crate::error::{VaultError, VaultResult};
use crate::key::VaultKey;

/// XChaCha20-Poly1305 extended nonce length in bytes.
pub const NONCE_LEN: usize = 24;
/// Poly1305 authentication tag length in bytes.
pub const TAG_LEN: usize = 16;

/// Encrypts `plaintext`, returning a fresh `(nonce, ciphertext || tag)` pair.
///
/// The plaintext buffer is staged inside `Zeroizing` so the intermediate copy is
/// wiped once the ciphertext exists.
pub fn seal(kek: &VaultKey, aad: &[u8], plaintext: &[u8]) -> VaultResult<(Vec<u8>, Vec<u8>)> {
    let cipher = XChaCha20Poly1305::new(Key::from_slice(kek.expose()));
    let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);

    let mut buffer = Zeroizing::new(plaintext.to_vec());
    let tag = cipher
        .encrypt_in_place_detached(&nonce, aad, buffer.as_mut_slice())
        .map_err(|_| VaultError::Crypto("authenticated encryption failed"))?;

    let mut ciphertext = Vec::with_capacity(buffer.len() + TAG_LEN);
    ciphertext.extend_from_slice(&buffer);
    ciphertext.extend_from_slice(&tag);

    Ok((nonce.to_vec(), ciphertext))
}

/// Decrypts and authenticates `ciphertext`, returning the plaintext in a
/// self-wiping buffer.
///
/// Any failure — wrong key, truncated input, modified ciphertext, modified AAD —
/// is reported as [`VaultError::Tampered`]. The error does not say which check
/// failed.
pub fn open(
    kek: &VaultKey,
    aad: &[u8],
    nonce: &[u8],
    ciphertext: &[u8],
) -> VaultResult<Zeroizing<Vec<u8>>> {
    if nonce.len() != NONCE_LEN || ciphertext.len() < TAG_LEN {
        return Err(VaultError::Tampered);
    }

    let cipher = XChaCha20Poly1305::new(Key::from_slice(kek.expose()));
    let (body, tag) = ciphertext.split_at(ciphertext.len() - TAG_LEN);
    let mut buffer = Zeroizing::new(body.to_vec());

    cipher
        .decrypt_in_place_detached(
            XNonce::from_slice(nonce),
            aad,
            buffer.as_mut_slice(),
            Tag::from_slice(tag),
        )
        .map_err(|_| VaultError::Tampered)?;

    Ok(buffer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn key() -> VaultKey {
        VaultKey::from_bytes([0x2a; 32])
    }

    #[test]
    fn round_trips_with_associated_data() {
        let (nonce, ciphertext) = seal(&key(), b"item:1", b"hunter2").expect("seal");
        let plaintext = open(&key(), b"item:1", &nonce, &ciphertext).expect("open");
        assert_eq!(plaintext.as_slice(), b"hunter2");
    }

    #[test]
    fn ciphertext_does_not_contain_the_plaintext() {
        let secret = b"correct-horse-battery-staple";
        let (nonce, ciphertext) = seal(&key(), b"item:1", secret).expect("seal");
        assert!(
            !ciphertext.windows(secret.len()).any(|window| window == secret),
            "plaintext must not appear anywhere in the ciphertext"
        );
        assert!(!nonce.windows(secret.len()).any(|window| window == secret));
    }

    #[test]
    fn flipping_any_ciphertext_bit_fails_authentication() {
        let (nonce, ciphertext) = seal(&key(), b"item:1", b"hunter2").expect("seal");
        for index in [0, ciphertext.len() / 2, ciphertext.len() - 1] {
            let mut modified = ciphertext.clone();
            modified[index] ^= 0x01;
            assert!(
                matches!(open(&key(), b"item:1", &nonce, &modified), Err(VaultError::Tampered)),
                "bit flip at byte {index} must be rejected"
            );
        }
    }

    #[test]
    fn associated_data_is_authenticated() {
        let (nonce, ciphertext) = seal(&key(), b"item:1", b"hunter2").expect("seal");
        assert!(matches!(
            open(&key(), b"item:2", &nonce, &ciphertext),
            Err(VaultError::Tampered)
        ));
    }

    #[test]
    fn a_different_key_fails_authentication() {
        let (nonce, ciphertext) = seal(&key(), b"item:1", b"hunter2").expect("seal");
        let other = VaultKey::from_bytes([0x2b; 32]);
        assert!(matches!(
            open(&other, b"item:1", &nonce, &ciphertext),
            Err(VaultError::Tampered)
        ));
    }

    #[test]
    fn truncated_input_is_rejected_without_panicking() {
        let (nonce, ciphertext) = seal(&key(), b"item:1", b"hunter2").expect("seal");
        assert!(open(&key(), b"item:1", &nonce, &ciphertext[..TAG_LEN - 1]).is_err());
        assert!(open(&key(), b"item:1", &nonce[..NONCE_LEN - 1], &ciphertext).is_err());
    }

    #[test]
    fn nonces_never_repeat() {
        let mut seen = HashSet::new();
        for _ in 0..2_000 {
            let (nonce, _) = seal(&key(), b"item:1", b"hunter2").expect("seal");
            assert!(seen.insert(nonce), "a nonce was reused under the same key");
        }
    }

    #[test]
    fn sealing_the_same_plaintext_twice_produces_different_ciphertext() {
        let first = seal(&key(), b"item:1", b"hunter2").expect("seal");
        let second = seal(&key(), b"item:1", b"hunter2").expect("seal");
        assert_ne!(first.0, second.0, "nonces must differ");
        assert_ne!(first.1, second.1, "ciphertext must differ");
    }
}
