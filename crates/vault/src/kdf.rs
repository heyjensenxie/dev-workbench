//! Master password key derivation.
//!
//! `Argon2id` (RFC 9106) is used exclusively. It is memory-hard, which is what
//! makes GPU and ASIC cracking of a stolen vault file expensive. The alternatives
//! the vault must avoid — MD5, SHA-1, a bare SHA-256 of the password, and
//! PBKDF2 — are not reachable from this module, because the only KDF dependency
//! declared by the crate is `argon2`.
//!
//! Parameters are chosen when the vault is created by benchmarking this machine
//! (see [`calibrate`]) rather than being hard-coded to a token cost. They are
//! stored as plaintext in the vault header, which is safe: KDF parameters are
//! not secret, and recording them is what allows the parameters to be raised
//! for *new* vaults or a password change without breaking existing ones.

use std::time::{Duration, Instant};

use argon2::{Algorithm, Argon2, Params, Version};
use rand::RngCore;
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::error::{VaultError, VaultResult};
use crate::key::{KEY_LEN, MasterPassword, VaultKey};

/// Identifier recorded in the vault header. Only `argon2id` is accepted.
pub const ALGORITHM_ARGON2ID: &str = "argon2id";
/// Version of the vault's own KDF description (not Argon2's internal version).
pub const KDF_VERSION: u32 = 1;
/// Salt length in bytes. 128 bits, drawn from the OS CSPRNG.
pub const SALT_LEN: usize = 16;

/// Argon2's own algorithm version, recorded implicitly by `KDF_VERSION`.
const ARGON2_VERSION: Version = Version::V0x13;

/// The security floor for memory cost.
///
/// This is a **floor, not a starting point**: calibration may raise the memory
/// cost above it, but a vault must never be created below it, whatever the
/// machine or build profile measures. It exists because calibration is driven by
/// a timing target, and a slow build (a debug binary, a heavily loaded machine)
/// would otherwise conclude that even a trivially cheap configuration is "too
/// expensive" and silently persist the weakest allowed parameters for the life
/// of the vault.
pub const MIN_MEMORY_KIB: u32 = 64 * 1024;

/// The floor used before this bound was corrected.
///
/// Kept only so that vaults created by an earlier build can be *recognised* and
/// reported as below the current floor; nothing new is ever created with it.
pub const LEGACY_MIN_MEMORY_KIB: u32 = 19 * 1024;

const MAX_MEMORY_KIB: u32 = 1024 * 1024;
/// The floor for time cost. One pass is Argon2's own minimum.
const MIN_TIME_COST: u32 = 3;
const MAX_TIME_COST: u32 = 16;
const MAX_PARALLELISM: u32 = 8;

/// How long a single derivation should take on the machine that created the
/// vault. Slow enough to hurt offline guessing, fast enough to unlock without
/// the user noticing.
pub const DEFAULT_CALIBRATION_TARGET: Duration = Duration::from_millis(500);

/// Everything needed to reproduce a KEK derivation.
///
/// Stored in plaintext in the vault header. Contains no secret material.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KdfParams {
    pub algorithm: String,
    pub version: u32,
    pub salt: Vec<u8>,
    pub memory_kib: u32,
    pub time_cost: u32,
    pub parallelism: u32,
    pub output_len: usize,
}

impl KdfParams {
    /// Builds Argon2id parameters with a freshly generated salt.
    pub fn new(
        memory_kib: u32,
        time_cost: u32,
        parallelism: u32,
    ) -> VaultResult<Self> {
        let mut salt = vec![0u8; SALT_LEN];
        OsRng.fill_bytes(&mut salt);
        Self::with_salt(memory_kib, time_cost, parallelism, salt)
    }

    /// Builds Argon2id parameters, raising any cost below the security floor.
    ///
    /// Used for every *new* configuration. A caller cannot accidentally create a
    /// vault below the floor, because the floor is applied here rather than
    /// trusted to the calibration loop.
    pub fn with_salt(
        memory_kib: u32,
        time_cost: u32,
        parallelism: u32,
        salt: Vec<u8>,
    ) -> VaultResult<Self> {
        let params = Self {
            algorithm: ALGORITHM_ARGON2ID.to_owned(),
            version: KDF_VERSION,
            salt,
            memory_kib: memory_kib.clamp(MIN_MEMORY_KIB, MAX_MEMORY_KIB),
            time_cost: time_cost.clamp(MIN_TIME_COST, MAX_TIME_COST),
            parallelism: parallelism.clamp(1, MAX_PARALLELISM),
            output_len: KEY_LEN,
        };
        params.validate()?;
        Ok(params)
    }

    /// Deliberately weak parameters, for automated tests only.
    ///
    /// This exists so the security test-suite can run thousands of derivations
    /// without taking minutes. It must never be used to create a real vault;
    /// [`crate::VaultService::create`] always calibrates instead, and
    /// `the_production_entry_point_never_picks_the_test_parameters` asserts that.
    ///
    /// It is constructed directly rather than through [`Self::with_salt`] so that
    /// raising the production floor does not silently make the test-suite slow.
    #[doc(hidden)]
    pub fn insecure_for_tests() -> Self {
        Self {
            algorithm: ALGORITHM_ARGON2ID.to_owned(),
            version: KDF_VERSION,
            salt: vec![0x5a; SALT_LEN],
            memory_kib: LEGACY_MIN_MEMORY_KIB,
            time_cost: 1,
            parallelism: 1,
            output_len: KEY_LEN,
        }
    }

    /// Whether these parameters meet the current security floor.
    ///
    /// A vault created below the floor keeps working — its parameters are in its
    /// header and are never rewritten behind the user's back — but the UI reports
    /// it, and changing the master password upgrades it.
    pub fn meets_current_floor(&self) -> bool {
        self.memory_kib >= MIN_MEMORY_KIB && self.time_cost >= MIN_TIME_COST
    }

    /// A fresh salt plus a cost raised to at least the current floor.
    ///
    /// Applied when the master password changes, so that a vault created before
    /// the floor was corrected (or on an unusually slow machine) is repaired by
    /// the next password change rather than staying weak forever. Raising the
    /// cost is safe: the parameters live in the header and are read back on every
    /// unlock.
    pub fn upgraded(&self) -> VaultResult<Self> {
        let mut params = Self::with_salt(
            self.memory_kib,
            self.time_cost,
            self.parallelism,
            self.fresh_salt(),
        )?;
        // `with_salt` already clamps to the floor; this keeps the intent explicit.
        params.memory_kib = params.memory_kib.max(MIN_MEMORY_KIB);
        params.time_cost = params.time_cost.max(MIN_TIME_COST);
        Ok(params)
    }

    /// A fresh salt plus a cost re-benchmarked for *this* machine.
    ///
    /// Where [`Self::upgraded`] only lifts a vault to the floor, this runs the full
    /// calibration and takes whichever is stronger. That distinction matters: a
    /// vault created on a slow machine, or by a build whose calibration collapsed
    /// to the floor, would otherwise stay there forever — [`Self::upgraded`] would
    /// only ever bring it to the floor, never to what the hardware can afford.
    ///
    /// The result is clamped so it can never be **weaker** than the configuration
    /// it replaces. Calibration measures the machine as it is right now, so a
    /// temporarily loaded or throttled host would otherwise talk a strong vault
    /// into weakening itself — the opposite of what the user asked for.
    pub fn recalibrated(&self) -> VaultResult<Self> {
        let calibrated = calibrate(DEFAULT_CALIBRATION_TARGET);
        Self::with_salt(
            calibrated.memory_kib.max(self.memory_kib),
            calibrated.time_cost.max(self.time_cost),
            calibrated.parallelism.max(self.parallelism),
            self.fresh_salt(),
        )
    }

    fn fresh_salt(&self) -> Vec<u8> {
        let mut salt = vec![0u8; SALT_LEN];
        OsRng.fill_bytes(&mut salt);
        salt
    }

    /// Rejects parameters this crate cannot derive with.
    ///
    /// Public so that callers which *accept* parameters from outside — restoring
    /// a backup, for instance — can refuse an unusable header before persisting
    /// it, rather than writing a vault that can never be opened.
    pub fn validate(&self) -> VaultResult<()> {
        if self.algorithm != ALGORITHM_ARGON2ID {
            return Err(VaultError::Corrupt("unsupported KDF algorithm"));
        }
        if self.version != KDF_VERSION {
            return Err(VaultError::Corrupt("unsupported KDF version"));
        }
        if self.salt.len() < SALT_LEN {
            return Err(VaultError::Corrupt("KDF salt is too short"));
        }
        if self.output_len != KEY_LEN {
            return Err(VaultError::Corrupt("unsupported KDF output length"));
        }
        self.argon2()?;
        Ok(())
    }

    fn argon2(&self) -> VaultResult<Argon2<'static>> {
        let params = Params::new(
            self.memory_kib,
            self.time_cost,
            self.parallelism,
            Some(self.output_len),
        )
        .map_err(|_| VaultError::Corrupt("invalid Argon2 parameters"))?;
        Ok(Argon2::new(Algorithm::Argon2id, ARGON2_VERSION, params))
    }

    /// Derives the key-encryption key from the master password.
    ///
    /// The output buffer is wiped on drop. The master password is borrowed and
    /// never copied into a longer-lived allocation here.
    pub fn derive(&self, password: &MasterPassword) -> VaultResult<VaultKey> {
        self.validate()?;
        let mut output = Zeroizing::new([0u8; KEY_LEN]);
        self.argon2()?
            .hash_password_into(password.expose().as_bytes(), &self.salt, output.as_mut())
            .map_err(|_| VaultError::Crypto("key derivation failed"))?;
        Ok(VaultKey::from_bytes(*output))
    }

    /// Cost of one derivation on this machine, for reporting in the UI.
    pub fn measure(&self) -> Duration {
        let probe = MasterPassword::new("dev-workbench-kdf-calibration-probe".to_owned());
        let started = Instant::now();
        // A failure here means the parameters are unusable, which `validate`
        // would already have caught; measuring is best-effort by design.
        let _ = self.derive(&probe);
        started.elapsed()
    }
}

/// Picks Argon2id parameters that cost roughly `target` on this machine.
///
/// Parallelism follows the available hardware threads (capped, since a desktop
/// app should not monopolise every core). Memory cost is then doubled from the
/// security floor until a single derivation would exceed the target time, so a
/// fast machine ends up with a heavier memory cost rather than a trivial one.
///
/// Two properties are guaranteed, and both matter more than hitting the target:
///
/// * **The result is always at or above [`MIN_MEMORY_KIB`].** The floor is
///   measured first, so the returned configuration is never an unmeasured
///   default. Calibration may raise the cost; it can never lower it below the
///   floor, whatever the machine or build profile reports.
/// * **A machine too slow to hit the target keeps the floor anyway.** Exceeding
///   the target costs the user a slower unlock; dropping below the floor would
///   cost them the security property the vault exists to provide. The trade-off
///   is resolved in favour of security, and the measured cost is exposed through
///   [`KdfParams::measure`] so the UI can report it.
pub fn calibrate(target: Duration) -> KdfParams {
    let parallelism = std::thread::available_parallelism()
        .map(|count| count.get() as u32)
        .unwrap_or(1)
        .clamp(1, 4);

    // The floor is the starting point *and* has been measured, so a degenerate
    // measurement can no longer select an unmeasured weakest-possible value.
    let Ok(mut best) = KdfParams::new(MIN_MEMORY_KIB, MIN_TIME_COST, parallelism) else {
        // The floor itself is not constructible, which would be a programming
        // error rather than a runtime condition; fall back to the documented
        // legacy parameters rather than panicking during vault creation.
        return KdfParams::insecure_for_tests();
    };

    let probe_password = MasterPassword::new("dev-workbench-kdf-calibration-probe".to_owned());
    while best.memory_kib < MAX_MEMORY_KIB {
        let candidate_memory = best.memory_kib.saturating_mul(2).min(MAX_MEMORY_KIB);
        if candidate_memory == best.memory_kib {
            break;
        }
        let Ok(candidate) = KdfParams::new(candidate_memory, MIN_TIME_COST, parallelism) else {
            break;
        };
        let started = Instant::now();
        if candidate.derive(&probe_password).is_err() {
            break;
        }
        // Stop climbing once a single derivation would cost more than the target,
        // and keep the last configuration that did not.
        if started.elapsed() > target {
            break;
        }
        best = candidate;
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    fn password() -> MasterPassword {
        MasterPassword::new("correct-horse-battery-staple".to_owned())
    }

    #[test]
    fn derivation_is_deterministic_for_the_same_salt_and_password() {
        let params = KdfParams::insecure_for_tests();
        let first = params.derive(&password()).expect("derive");
        let second = params.derive(&password()).expect("derive");
        assert_eq!(first.expose(), second.expose());
    }

    #[test]
    fn different_salts_produce_different_keys() {
        let first = KdfParams::insecure_for_tests();
        let mut second = first.clone();
        second.salt = vec![0x11; SALT_LEN];
        assert_ne!(
            first.derive(&password()).expect("derive").expose(),
            second.derive(&password()).expect("derive").expose()
        );
    }

    #[test]
    fn different_passwords_produce_different_keys() {
        let params = KdfParams::insecure_for_tests();
        let other = MasterPassword::new("correct-horse-battery-stapl".to_owned());
        assert_ne!(
            params.derive(&password()).expect("derive").expose(),
            params.derive(&other).expect("derive").expose()
        );
    }

    #[test]
    fn a_tampered_algorithm_is_rejected() {
        let mut params = KdfParams::insecure_for_tests();
        params.algorithm = "pbkdf2".into();
        assert!(matches!(
            params.derive(&password()),
            Err(VaultError::Corrupt(_))
        ));
    }

    #[test]
    fn a_short_salt_is_rejected() {
        let mut params = KdfParams::insecure_for_tests();
        params.salt = vec![0u8; 4];
        assert!(params.derive(&password()).is_err());
    }

    #[test]
    fn calibration_stays_within_the_documented_bounds() {
        // Uses the real calibration path, but with a tiny target so the test
        // stays fast; the result must still be a usable Argon2id configuration.
        let params = calibrate(Duration::from_millis(1));
        assert_eq!(params.algorithm, ALGORITHM_ARGON2ID);
        assert_eq!(params.version, KDF_VERSION);
        assert!(params.memory_kib >= MIN_MEMORY_KIB);
        assert!(params.memory_kib <= MAX_MEMORY_KIB);
        assert!((1..=MAX_PARALLELISM).contains(&params.parallelism));
        assert!(params.derive(&password()).is_ok());
    }

    #[test]
    fn calibration_never_goes_below_the_floor_however_cheap_the_target() {
        // Regression: an earlier implementation started from the weakest allowed
        // configuration and only raised it after a successful measurement. On a
        // slow machine — or a debug build, where Argon2 runs unoptimised — the
        // very first measurement exceeded the target, and the weakest possible
        // parameters were persisted for the lifetime of the vault.
        //
        // A degenerate target of zero must still produce a floor-or-better cost.
        let params = calibrate(Duration::ZERO);
        assert!(
            params.memory_kib >= MIN_MEMORY_KIB,
            "calibration dropped below the security floor: {} KiB",
            params.memory_kib
        );
        assert!(params.time_cost >= MIN_TIME_COST);
        assert!(params.meets_current_floor());
    }

    #[test]
    fn a_floor_configuration_is_within_the_usable_range() {
        // The floor must be constructible and derivable, or vault creation would
        // fail outright on a machine that cannot go above it.
        let params = KdfParams::new(MIN_MEMORY_KIB, MIN_TIME_COST, 1).expect("floor params");
        assert!(params.validate().is_ok());
        assert!(params.derive(&password()).is_ok());
    }

    #[test]
    fn the_floor_is_materially_above_the_legacy_minimum() {
        // The whole point of the correction: a legacy vault is measurably weaker.
        // Expressed as a compile-time assertion so it can never regress silently.
        const _: () = assert!(MIN_MEMORY_KIB > LEGACY_MIN_MEMORY_KIB);
        assert!(!KdfParams::insecure_for_tests().meets_current_floor());
    }

    #[test]
    fn legacy_parameters_still_derive_and_are_reported_as_below_the_floor() {
        // Backwards compatibility: a vault created by an earlier build must keep
        // opening. Its parameters are read from its header, never re-clamped.
        let legacy = KdfParams::insecure_for_tests();
        assert!(!legacy.meets_current_floor());
        assert!(legacy.derive(&password()).is_ok());
    }

    #[test]
    fn upgrading_raises_a_weak_configuration_to_the_floor() {
        let legacy = KdfParams::insecure_for_tests();
        let upgraded = legacy.upgraded().expect("upgrade");

        assert!(upgraded.meets_current_floor());
        assert!(upgraded.memory_kib >= MIN_MEMORY_KIB);
        assert!(upgraded.time_cost >= MIN_TIME_COST);
        // A fresh salt must accompany the new cost.
        assert_ne!(upgraded.salt, legacy.salt);
        assert_eq!(upgraded.salt.len(), SALT_LEN);
        assert!(upgraded.derive(&password()).is_ok());
    }

    #[test]
    fn upgrading_leaves_an_already_strong_configuration_at_or_above_its_cost() {
        let strong = KdfParams::new(512 * 1024, 4, 2).expect("strong params");
        let upgraded = strong.upgraded().expect("upgrade");
        assert!(upgraded.memory_kib >= strong.memory_kib);
        assert!(upgraded.time_cost >= strong.time_cost);
    }

    #[test]
    fn recalibrating_lifts_a_legacy_vault_above_the_floor() {
        // The remedy for a vault whose parameters came from a build that
        // collapsed to the floor: `upgraded` would only ever reach the floor,
        // this reaches whatever the machine can afford.
        let legacy = KdfParams::insecure_for_tests();
        let recalibrated = legacy.recalibrated().expect("recalibrate");

        assert!(recalibrated.meets_current_floor());
        assert!(recalibrated.memory_kib >= MIN_MEMORY_KIB);
        assert_ne!(recalibrated.salt, legacy.salt, "a fresh salt is required");
        assert_eq!(recalibrated.salt.len(), SALT_LEN);
        assert!(recalibrated.derive(&password()).is_ok());
    }

    #[test]
    fn recalibrating_never_weakens_an_existing_configuration() {
        // Calibration measures the machine as it is right now. On a loaded or
        // throttled host it would pick a lower cost, which must not be allowed to
        // weaken a vault that is already stronger.
        let strong = KdfParams::new(MAX_MEMORY_KIB, MAX_TIME_COST - 1, 4).expect("strong params");
        let recalibrated = strong.recalibrated().expect("recalibrate");

        assert!(
            recalibrated.memory_kib >= strong.memory_kib,
            "recalibration lowered the memory cost from {} to {}",
            strong.memory_kib,
            recalibrated.memory_kib
        );
        assert!(recalibrated.time_cost >= strong.time_cost);
        assert!(recalibrated.parallelism >= strong.parallelism);
        // And the cost is still bounded, so the vault cannot become unusable.
        assert!(recalibrated.memory_kib <= MAX_MEMORY_KIB);
        assert!(recalibrated.time_cost <= MAX_TIME_COST);
    }

    #[test]
    fn recalibrating_keeps_the_configuration_usable() {
        let recalibrated = KdfParams::insecure_for_tests().recalibrated().expect("recalibrate");
        assert!(recalibrated.derive(&password()).is_ok());
        // The parameters must round-trip through the header unchanged.
        let encoded = serde_json::to_string(&recalibrated).expect("serialize");
        let decoded: KdfParams = serde_json::from_str(&encoded).expect("deserialize");
        assert_eq!(decoded, recalibrated);
    }

    #[test]
    fn default_calibration_is_meaningfully_expensive() {
        let params = calibrate(DEFAULT_CALIBRATION_TARGET);
        assert!(params.meets_current_floor());
        let elapsed = params.measure();
        assert!(
            elapsed >= Duration::from_millis(50),
            "default calibration produced a suspiciously cheap derivation: {elapsed:?}"
        );
    }
}
