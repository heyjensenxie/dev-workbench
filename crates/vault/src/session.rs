//! Unlocked-vault session state and automatic locking.
//!
//! The vault master key exists only here, only in process memory, and only for
//! as long as the vault is unlocked. It is never written to disk, never sent
//! across the Tauri IPC boundary to the WebView, and never placed in the OS
//! credential store. Locking drops the key, which wipes it.
//!
//! Locking is triggered by more than a timer. A local vault is unlocked while
//! the user is at the machine, so every signal that the user may have left is
//! treated as a reason to lock:
//!
//! * the idle timeout (configurable, 1 to 30 minutes, default 5);
//! * an explicit `lock` call (the UI's Lock button and its shortcut);
//! * the application closing;
//! * the process resuming after the machine was suspended — see
//!   [`VaultSession::detect_resume_from_sleep`]. A monotonic clock does not
//!   advance while a machine sleeps, so an idle timer alone would let a laptop
//!   sit closed for a week and still be unlocked on wake. Comparing wall-clock
//!   progress against monotonic progress detects that case directly.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime};

use tokio::sync::RwLock;

use crate::key::VaultKey;

/// Auto-lock choices offered by the UI, in seconds.
///
/// "Never" is intentionally not offered. A local password manager that stays
/// unlocked indefinitely is the failure mode this module exists to prevent.
pub const AUTO_LOCK_OPTIONS_SECONDS: [u64; 4] = [60, 300, 600, 1800];
/// Default idle timeout.
pub const DEFAULT_AUTO_LOCK_SECONDS: u64 = 300;

/// Wall-clock advance beyond monotonic advance that indicates the machine slept.
const SLEEP_DETECTION_THRESHOLD: Duration = Duration::from_secs(25);

struct ClockAnchor {
    monotonic: Instant,
    wall: SystemTime,
}

pub struct VaultSession {
    key: RwLock<Option<VaultKey>>,
    started: Instant,
    last_activity_ms: AtomicU64,
    auto_lock_seconds: AtomicU64,
    clock: Mutex<ClockAnchor>,
}

impl Default for VaultSession {
    fn default() -> Self {
        Self::new()
    }
}

impl VaultSession {
    pub fn new() -> Self {
        Self {
            key: RwLock::new(None),
            started: Instant::now(),
            last_activity_ms: AtomicU64::new(0),
            auto_lock_seconds: AtomicU64::new(DEFAULT_AUTO_LOCK_SECONDS),
            clock: Mutex::new(ClockAnchor {
                monotonic: Instant::now(),
                wall: SystemTime::now(),
            }),
        }
    }

    fn now_ms(&self) -> u64 {
        u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    /// Marks user activity, pushing the idle deadline out.
    pub fn touch(&self) {
        self.last_activity_ms
            .store(self.now_ms(), Ordering::Relaxed);
    }

    pub fn idle_millis(&self) -> u64 {
        self.now_ms()
            .saturating_sub(self.last_activity_ms.load(Ordering::Relaxed))
    }

    pub fn auto_lock_seconds(&self) -> u64 {
        self.auto_lock_seconds.load(Ordering::Relaxed)
    }

    pub fn set_auto_lock_seconds(&self, seconds: u64) {
        let seconds = if seconds == 0 {
            DEFAULT_AUTO_LOCK_SECONDS
        } else {
            seconds
        };
        self.auto_lock_seconds
            .store(seconds.min(24 * 60 * 60), Ordering::Relaxed);
    }

    /// True once the idle timeout has elapsed.
    pub fn idle_timeout_reached(&self) -> bool {
        let limit = self.auto_lock_seconds().saturating_mul(1_000);
        limit > 0 && self.idle_millis() >= limit
    }

    pub async fn is_unlocked(&self) -> bool {
        self.key.read().await.is_some()
    }

    /// Whether the live vault master key was pinned out of the page file.
    ///
    /// Does not count as activity: reporting a fact about the key must not extend
    /// the idle deadline.
    pub async fn is_key_memory_locked(&self) -> bool {
        self.key
            .read()
            .await
            .as_ref()
            .is_some_and(VaultKey::is_memory_locked)
    }

    /// Installs a freshly unwrapped vault master key.
    pub async fn unlock(&self, key: VaultKey) {
        *self.key.write().await = Some(key);
        self.touch();
    }

    /// Drops the vault master key (wiping it) and resets failure accounting.
    ///
    /// Returns whether a key was actually dropped, so callers can avoid
    /// emitting a redundant "locked" event.
    pub async fn lock(&self) -> bool {
        let dropped = self.key.write().await.take();
        self.touch();
        dropped.is_some()
    }

    /// Returns the vault master key, or `None` when the vault is locked.
    ///
    /// The idle timeout is re-checked here as well as by the background sweeper,
    /// so a command can never operate on a key whose deadline has already
    /// passed. The returned key is an independently-wiped copy.
    pub async fn key(&self) -> Option<VaultKey> {
        if self.idle_timeout_reached() {
            self.lock().await;
            return None;
        }
        self.touch();
        self.key.read().await.as_ref().cloned()
    }

    /// Locks if the idle timeout has elapsed. Called periodically by the app.
    pub async fn sweep(&self) -> bool {
        if self.idle_timeout_reached() {
            return self.lock().await;
        }
        false
    }

    /// True when the process appears to have resumed after the machine slept.
    ///
    /// Compares how far the wall clock moved against how far a monotonic clock
    /// moved over the same interval. Suspension freezes the monotonic clock but
    /// not the wall clock, so a large gap means the machine was asleep. The
    /// anchor is refreshed on every call so a single resume is reported once.
    pub fn detect_resume_from_sleep(&self) -> bool {
        let Ok(mut anchor) = self.clock.lock() else {
            return false;
        };
        let monotonic_delta = anchor.monotonic.elapsed();
        let wall_delta = anchor.wall.elapsed().unwrap_or_default();
        anchor.monotonic = Instant::now();
        anchor.wall = SystemTime::now();
        wall_delta > monotonic_delta + SLEEP_DETECTION_THRESHOLD
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> VaultKey {
        VaultKey::generate()
    }

    #[tokio::test]
    async fn a_new_session_is_locked() {
        let session = VaultSession::new();
        assert!(!session.is_unlocked().await);
        assert!(session.key().await.is_none());
    }

    #[tokio::test]
    async fn locking_drops_the_key() {
        let session = VaultSession::new();
        session.unlock(key()).await;
        assert!(session.is_unlocked().await);
        assert!(session.lock().await);
        assert!(!session.is_unlocked().await);
        assert!(session.key().await.is_none());
        assert!(!session.lock().await, "locking a locked vault is a no-op");
    }

    #[tokio::test]
    async fn the_idle_timeout_locks_the_session() {
        let session = VaultSession::new();
        session.unlock(key()).await;
        session.set_auto_lock_seconds(1);
        assert!(!session.idle_timeout_reached());
        // Nothing is touched, so the deadline passes.
        std::thread::sleep(Duration::from_millis(1_100));
        assert!(session.idle_timeout_reached());
        assert!(session.sweep().await);
        assert!(!session.is_unlocked().await);
    }

    #[tokio::test]
    async fn activity_pushes_the_deadline_out() {
        let session = VaultSession::new();
        session.unlock(key()).await;
        session.set_auto_lock_seconds(1);
        for _ in 0..4 {
            std::thread::sleep(Duration::from_millis(300));
            let _ = session.key().await;
        }
        assert!(!session.idle_timeout_reached());
        assert!(session.is_unlocked().await);
    }

    #[tokio::test]
    async fn reading_the_key_after_expiry_locks_instead_of_serving_it() {
        let session = VaultSession::new();
        session.unlock(key()).await;
        session.set_auto_lock_seconds(1);
        std::thread::sleep(Duration::from_millis(1_100));
        assert!(session.key().await.is_none());
        assert!(!session.is_unlocked().await);
    }

    #[test]
    fn an_untouched_clock_is_not_reported_as_a_resume() {
        let session = VaultSession::new();
        assert!(!session.detect_resume_from_sleep());
    }

    #[test]
    fn auto_lock_seconds_reject_zero() {
        let session = VaultSession::new();
        session.set_auto_lock_seconds(0);
        assert_eq!(session.auto_lock_seconds(), DEFAULT_AUTO_LOCK_SECONDS);
    }
}
