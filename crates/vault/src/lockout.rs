//! Unlock throttling.
//!
//! ## What this does and does not do
//!
//! This is an **interactive** throttle. It stops someone sitting at an unlocked
//! machine from working through a list of guesses against the unlock prompt.
//!
//! It is **not** protection against an offline attack, and must never be
//! presented as such. An attacker who copies `vault.db` runs their guesses
//! outside this application entirely and never touches this code; the counter
//! below lives in the vault header, in plaintext, and they can simply ignore or
//! reset it. The defences that matter against a stolen file are a strong master
//! password and the cost of Argon2id — see [`crate::kdf`].
//!
//! ## Shape of the policy
//!
//! * The first [`FREE_ATTEMPTS`] failures do not lock the vault. Mistyping a
//!   passphrase a couple of times is normal, and each attempt already costs a
//!   full Argon2id derivation — a quarter of a second on the machine measured
//!   during development — so there is no separate per-attempt delay to add.
//! * The [`LOCKOUT_THRESHOLD`]-th failure locks the vault for
//!   [`BASE_LOCKOUT_SECONDS`], and each further failure doubles it.
//! * The escalation is **capped**. This is a deliberate trade-off: there is no
//!   recovery path for a master password, so an uncapped policy would let a
//!   curious colleague — or the owner fumbling a passphrase — lock the vault for
//!   days with no way out. A one-hour ceiling still allows only 24 guesses a day,
//!   which is worthless for guessing a real passphrase, while keeping the vault
//!   reachable by the person who owns it.
//!
//! The counter is only reset by a **successful unlock**. An expired lockout does
//! not reset it, which is what makes later failures escalate.

/// Failures allowed before the vault locks at all.
pub const FREE_ATTEMPTS: u32 = 4;

/// The failure at which the vault starts locking. Shared with the UI, which
/// reveals the recovery actions at the same point.
pub const LOCKOUT_THRESHOLD: u32 = 5;

/// How long the vault locks on the [`LOCKOUT_THRESHOLD`]-th failure.
pub const BASE_LOCKOUT_SECONDS: u64 = 300;

/// Ceiling on the escalation, so the owner cannot lock themselves out for days.
pub const MAX_LOCKOUT_SECONDS: u64 = 3600;

/// How long the vault stays locked after `failed_attempts` consecutive failures.
///
/// Returns zero below the threshold. Doubles with each failure past it, up to
/// [`MAX_LOCKOUT_SECONDS`].
pub fn lockout_seconds(failed_attempts: u32) -> u64 {
    if failed_attempts < LOCKOUT_THRESHOLD {
        return 0;
    }
    let steps = u32::min(failed_attempts - LOCKOUT_THRESHOLD, 16);
    BASE_LOCKOUT_SECONDS
        .saturating_mul(1u64 << steps)
        .min(MAX_LOCKOUT_SECONDS)
}

/// Whether a lockout of [`lockout_seconds`] starts at this failure.
pub fn starts_lockout(failed_attempts: u32) -> bool {
    lockout_seconds(failed_attempts) > 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn early_failures_do_not_lock_the_vault() {
        for attempts in 0..=FREE_ATTEMPTS {
            assert_eq!(
                lockout_seconds(attempts),
                0,
                "{attempts} failures must not lock the vault"
            );
            assert!(!starts_lockout(attempts));
        }
    }

    #[test]
    fn the_threshold_failure_locks_for_five_minutes() {
        assert_eq!(lockout_seconds(LOCKOUT_THRESHOLD), 300);
        assert!(starts_lockout(LOCKOUT_THRESHOLD));
    }
    #[test]
    fn later_failures_escalate() {
        let fifth = lockout_seconds(5);
        let sixth = lockout_seconds(6);
        let seventh = lockout_seconds(7);
        assert!(sixth > fifth, "each further failure must escalate");
        assert!(seventh > sixth);
        assert_eq!(sixth, fifth * 2);
        assert_eq!(seventh, fifth * 4);
    }

    #[test]
    fn the_escalation_is_capped_so_the_owner_is_never_locked_out_permanently() {
        // There is no master-password recovery, so an uncapped policy would let
        // the vault become permanently unreachable.
        for attempts in [10, 20, 50, 1_000, u32::MAX] {
            let seconds = lockout_seconds(attempts);
            assert!(
                seconds <= MAX_LOCKOUT_SECONDS,
                "{attempts} failures produced an uncapped lockout of {seconds}s"
            );
        }
        assert_eq!(lockout_seconds(u32::MAX), MAX_LOCKOUT_SECONDS);
    }

    #[test]
    fn the_cap_still_limits_guessing_to_a_handful_per_day() {
        // The whole point of allowing a high cap: even at the ceiling, the number
        // of guesses per day stays negligible.
        let per_day = 24 * 60 * 60 / MAX_LOCKOUT_SECONDS;
        assert!(per_day <= 24, "capped lockout allows {per_day} guesses a day");
    }

    #[test]
    fn the_free_attempts_leave_room_for_an_honest_mistake() {
        // Someone fumbling a long passphrase must not be locked out immediately.
        // Enforced at compile time so the policy cannot drift unnoticed.
        const _: () = assert!(FREE_ATTEMPTS >= 3);
        const _: () = assert!(FREE_ATTEMPTS < LOCKOUT_THRESHOLD);
    }
}
