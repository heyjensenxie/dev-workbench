//! Password generation.
//!
//! Randomness comes from [`rand::rngs::OsRng`], which reads the operating
//! system's cryptographically secure generator (`RtlGenRandom`/`BCryptGenRandom`
//! on Windows, `getrandom(2)` on Linux). A non-cryptographic generator such as
//! `Math.random`, a seeded PRNG, or a timestamp is not reachable from here.
//!
//! Generated passwords are returned to the caller and nothing else. They are not
//! logged, not stored, not added to any history, and not persisted until the
//! user explicitly applies one to an item.

use rand::RngCore;
use rand::rngs::OsRng;
use rand::seq::SliceRandom;
use secrecy::SecretString;
use serde::Deserialize;
use zeroize::Zeroize;

use crate::error::{VaultError, VaultResult};

const LOWERCASE: &str = "abcdefghijklmnopqrstuvwxyz";
const UPPERCASE: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const NUMBERS: &str = "0123456789";
const SYMBOLS: &str = "!@#$%^&*()-_=+[]{};:,.?/~|";
/// Characters that are easy to confuse when a password is transcribed by hand.
const AMBIGUOUS: &str = "Il1O0o|";

pub const MIN_LENGTH: usize = 8;
pub const MAX_LENGTH: usize = 128;
pub const DEFAULT_LENGTH: usize = 20;

/// A short default would silently weaken every password generated without the
/// user opening the length control. Enforced at compile time.
const _: () = assert!(
    DEFAULT_LENGTH >= 20,
    "the default generator length must stay at least 20 characters"
);

/// Password shape requested by the UI.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratorOptions {
    pub length: usize,
    pub uppercase: bool,
    pub lowercase: bool,
    pub numbers: bool,
    pub symbols: bool,
    pub exclude_ambiguous: bool,
}

impl Default for GeneratorOptions {
    fn default() -> Self {
        Self {
            length: DEFAULT_LENGTH,
            uppercase: true,
            lowercase: true,
            numbers: true,
            symbols: true,
            exclude_ambiguous: true,
        }
    }
}

/// Builds a password from the enabled character classes.
///
/// At least one character is drawn from every enabled class, then the result is
/// shuffled, so an enabled class is never silently absent from the output.
pub fn generate(options: &GeneratorOptions) -> VaultResult<SecretString> {
    let length = options.length.clamp(MIN_LENGTH, MAX_LENGTH);

    let mut classes: Vec<Vec<char>> = Vec::new();
    for (enabled, source) in [
        (options.lowercase, LOWERCASE),
        (options.uppercase, UPPERCASE),
        (options.numbers, NUMBERS),
        (options.symbols, SYMBOLS),
    ] {
        if !enabled {
            continue;
        }
        let alphabet = alphabet(source, options.exclude_ambiguous);
        if !alphabet.is_empty() {
            classes.push(alphabet);
        }
    }
    if classes.is_empty() {
        return Err(VaultError::Validation(
            "at least one character set must be enabled".into(),
        ));
    }

    let mut rng = OsRng;
    let mut generated: Vec<char> = Vec::with_capacity(length);
    for class in &classes {
        if let Some(character) = class.choose(&mut rng) {
            generated.push(*character);
        }
    }
    let pool: Vec<char> = classes.concat();
    while generated.len() < length {
        match pool.choose(&mut rng) {
            Some(character) => generated.push(*character),
            None => break,
        }
    }
    generated.shuffle(&mut rng);

    let password: String = generated.iter().collect();
    generated.zeroize();

    // The only copy handed out is owned by a self-wiping `SecretString`.
    Ok(SecretString::from(password))
}

fn alphabet(source: &str, exclude_ambiguous: bool) -> Vec<char> {
    source
        .chars()
        .filter(|character| !exclude_ambiguous || !AMBIGUOUS.contains(*character))
        .collect()
}

/// Draws `count` random bytes from the OS CSPRNG, for callers that need salts.
pub fn random_bytes(count: usize) -> Vec<u8> {
    let mut buffer = vec![0u8; count];
    OsRng.fill_bytes(&mut buffer);
    buffer
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::ExposeSecret;
    use std::collections::HashSet;

    fn generate_string(options: &GeneratorOptions) -> String {
        generate(options)
            .expect("generate")
            .expose_secret()
            .to_owned()
    }

    #[test]
    fn respects_the_requested_length() {
        for length in [MIN_LENGTH, 20, 64, MAX_LENGTH] {
            let options = GeneratorOptions {
                length,
                ..Default::default()
            };
            assert_eq!(generate_string(&options).chars().count(), length);
        }
    }

    #[test]
    fn clamps_out_of_range_lengths() {
        let too_short = generate_string(&GeneratorOptions {
            length: 1,
            ..Default::default()
        });
        assert_eq!(too_short.chars().count(), MIN_LENGTH);
        let too_long = generate_string(&GeneratorOptions {
            length: 10_000,
            ..Default::default()
        });
        assert_eq!(too_long.chars().count(), MAX_LENGTH);
    }

    #[test]
    fn every_enabled_class_appears_at_least_once() {
        let options = GeneratorOptions {
            length: 8,
            ..Default::default()
        };
        for _ in 0..200 {
            let password = generate_string(&options);
            assert!(password.chars().any(|c| c.is_ascii_lowercase()));
            assert!(password.chars().any(|c| c.is_ascii_uppercase()));
            assert!(password.chars().any(|c| c.is_ascii_digit()));
            assert!(password.chars().any(|c| SYMBOLS.contains(c)));
        }
    }

    #[test]
    fn disabled_classes_never_appear() {
        let options = GeneratorOptions {
            length: 40,
            uppercase: false,
            numbers: false,
            symbols: false,
            ..Default::default()
        };
        let password = generate_string(&options);
        assert!(password.chars().all(|c| c.is_ascii_lowercase()));
    }

    #[test]
    fn rejecting_every_class_is_an_error() {
        let options = GeneratorOptions {
            uppercase: false,
            lowercase: false,
            numbers: false,
            symbols: false,
            ..Default::default()
        };
        assert!(matches!(generate(&options), Err(VaultError::Validation(_))));
    }

    #[test]
    fn ambiguous_characters_can_be_excluded() {
        let options = GeneratorOptions {
            length: 128,
            exclude_ambiguous: true,
            ..Default::default()
        };
        for _ in 0..50 {
            let password = generate_string(&options);
            assert!(password.chars().all(|c| !AMBIGUOUS.contains(c)));
        }
    }

    #[test]
    fn generated_passwords_do_not_repeat() {
        let options = GeneratorOptions::default();
        let mut seen = HashSet::new();
        for _ in 0..500 {
            assert!(
                seen.insert(generate_string(&options)),
                "the generator produced a duplicate password"
            );
        }
    }

    #[test]
    fn the_default_length_is_generous() {
        assert_eq!(
            generate_string(&GeneratorOptions::default())
                .chars()
                .count(),
            DEFAULT_LENGTH
        );
    }
}
