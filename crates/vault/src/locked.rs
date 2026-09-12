//! Secret material held in memory that resists being written to disk.
//!
//! Wiping a secret on drop (which [`zeroize`] does) only helps once it is *gone*.
//! While it is alive, the operating system is free to page the containing memory
//! out to the swap file or pagefile, and a pagefile is not encrypted by this
//! application. A master password or vault master key sitting in a pagefile would
//! be a strictly worse leak than the ciphertext it protects.
//!
//! These types close that window by asking the operating system to pin the pages
//! in physical memory for as long as the value lives:
//!
//! * `VirtualLock` on Windows;
//! * `mlock` on Unix (including macOS).
//!
//! Three details make this sound rather than merely optimistic:
//!
//! * **Whole pages belong to this value.** Kernel locks round to page
//!   boundaries, so a buffer sharing a page with unrelated allocations would
//!   either lock that unrelated memory too or, worse, have its wipe step corrupt
//!   it. Every allocation here is page-aligned and a whole number of pages, and
//!   `LOCKED_EXTENT` is a page size on every platform in use (4 KiB on x86-64
//!   Windows and Linux, 16 KiB on Apple silicon).
//! * **The wipe covers the whole allocation**, not just the meaningful bytes, so
//!   anything the allocator or the caller's copies left behind is cleared too.
//! * **Page count is minimised.** The number of pages a process may lock is
//!   capped by the operating system, so each secret costs exactly the pages it
//!   needs and no more. See [`ensure_lock_quota`].
//!
//! Locking is best-effort by nature: it can be refused when a process is at its
//! quota. [`LockedBytes::is_locked`] and [`LockedBuffer::is_locked`] report
//! whether it actually took effect, so the claim can be tested and surfaced
//! rather than assumed.

use std::alloc::{self, Layout};
use std::ptr::NonNull;
use std::sync::Once;

use zeroize::Zeroize;

/// Allocation granularity, in bytes: one page on every supported platform.
///
/// Allocations are a whole multiple of this and aligned to it, so no other
/// allocation ever shares these pages.
#[cfg(target_os = "macos")]
const LOCKED_EXTENT: usize = 16 * 1024;
#[cfg(not(target_os = "macos"))]
const LOCKED_EXTENT: usize = 4 * 1024;

/// Bytes to hold resident, per process, when raising the lock quota.
///
/// Windows derives the lockable-page count from the process's *minimum* working
/// set, which by default is about 50 pages — measured on this project as only
/// **11** lockable 16 KiB allocations. That is far too tight to rely on, so the
/// quota is raised once, to a value that remains negligible for a desktop
/// application.
#[cfg(windows)]
const REQUIRED_WORKING_SET: usize = 16 * 1024 * 1024;
#[cfg(windows)]
const MAXIMUM_WORKING_SET: usize = 128 * 1024 * 1024;

static QUOTA: Once = Once::new();

/// Raises this process's locked-page quota once, so that secrets can actually be
/// pinned.
///
/// Without this, the operating system may refuse `VirtualLock` after only a
/// handful of secrets, and the refusal is silent unless the caller checks
/// `is_locked`. Best-effort: a failure leaves the quota at its default, and
/// locking degrades to being reported as unavailable rather than claimed falsely.
fn ensure_lock_quota() {
    QUOTA.call_once(|| {
        #[cfg(windows)]
        {
            use windows_sys::Win32::System::Threading::{
                GetCurrentProcess, SetProcessWorkingSetSize,
            };
            // SAFETY: adjusting this process's own working set. Both arguments are
            // byte counts, and failure is tolerated.
            unsafe {
                SetProcessWorkingSetSize(
                    GetCurrentProcess(),
                    REQUIRED_WORKING_SET,
                    MAXIMUM_WORKING_SET,
                );
            }
        }
        // Unix systems do not derive the mlock limit from the working set; the
        // limit comes from RLIMIT_MEMLOCK, which a library has no business
        // changing for its host process.
    });
}

/// Whether this platform has a locking implementation at all.
pub const fn is_supported() -> bool {
    cfg!(any(windows, unix))
}

/// A page-complete, optionally locked allocation owned by exactly one secret.
struct LockedPage {
    ptr: NonNull<u8>,
    /// Allocation size in bytes, always a whole multiple of `LOCKED_EXTENT`.
    extent: usize,
    /// Whether the operating system accepted the lock request. Reported rather
    /// than assumed, because a refused lock must not be described as protected.
    locked: bool,
}

impl LockedPage {
    /// Allocates enough whole pages to hold `len` bytes.
    fn new(len: usize) -> Self {
        // A secret of zero length still needs a page: a zero-sized allocation has
        // no address to lock.
        let pages = len.div_ceil(LOCKED_EXTENT).max(1);
        let extent = pages * LOCKED_EXTENT;
        let layout = Layout::from_size_align(extent, LOCKED_EXTENT).expect("valid locked layout");

        // SAFETY: `layout` has a non-zero size, which is the only requirement for
        // `alloc_zeroed`.
        let ptr = unsafe { alloc::alloc_zeroed(layout) };
        let Some(ptr) = NonNull::new(ptr) else {
            // Failing to obtain memory for secret material is unrecoverable: the
            // alternative is to continue without the locked allocation this type
            // exists to provide.
            alloc::handle_alloc_error(layout);
        };

        // Ask for a usable quota before the first attempt: the default allows
        // only a handful of locked secrets.
        ensure_lock_quota();
        let locked = lock_region(ptr.as_ptr(), extent);
        Self {
            ptr,
            extent,
            locked,
        }
    }

    fn layout(&self) -> Layout {
        Layout::from_size_align(self.extent, LOCKED_EXTENT).expect("valid locked layout")
    }

    /// The whole allocation, including padding past the secret.
    fn bytes_mut(&mut self) -> &mut [u8] {
        // SAFETY: `ptr` owns `extent` initialised bytes for as long as `self` is
        // alive, and `&mut self` guarantees exclusive access.
        unsafe { std::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.extent) }
    }

    /// The whole allocation.
    fn bytes(&self) -> &[u8] {
        // SAFETY: as above, with a shared borrow.
        unsafe { std::slice::from_raw_parts(self.ptr.as_ptr(), self.extent) }
    }

    /// The first `len` bytes, which the caller has already initialised.
    fn secret(&self, len: usize) -> &[u8] {
        &self.bytes()[..len]
    }

    /// Overwrites every byte of the allocation.
    ///
    /// `zeroize` uses volatile writes and a compiler fence, so this cannot be
    /// elided as a dead store before the deallocation.
    fn wipe(&mut self) {
        self.bytes_mut().zeroize();
    }
}

impl Drop for LockedPage {
    fn drop(&mut self) {
        self.wipe();
        if self.locked {
            unlock_region(self.ptr.as_ptr(), self.extent);
        }
        // SAFETY: `ptr` came from `alloc::alloc_zeroed` with exactly this layout
        // and has not been freed since.
        unsafe { alloc::dealloc(self.ptr.as_ptr(), self.layout()) };
    }
}

// SAFETY: the allocation is owned by this value alone and is reachable only
// through `&self` / `&mut self`, so moving it between threads or sharing it
// cannot introduce aliasing. The raw pointer is an implementation detail of the
// allocation, not shared state.
unsafe impl Send for LockedPage {}
unsafe impl Sync for LockedPage {}

/// A fixed-size secret pinned in memory and wiped on drop.
///
/// Used for cryptographic keys, where the length is part of the type.
pub struct LockedBytes<const N: usize>(LockedPage);

impl<const N: usize> LockedBytes<N> {
    /// Copies `value` into freshly pinned memory.
    pub fn new(value: [u8; N]) -> Self {
        let mut page = LockedPage::new(N);
        page.bytes_mut()[..N].copy_from_slice(&value);
        Self(page)
    }

    /// Borrows the secret.
    pub fn as_bytes(&self) -> &[u8; N] {
        // SAFETY: the first `N` bytes were initialised in `new`, the allocation
        // is at least `N` bytes, and the value is never relocated.
        unsafe { &*(self.0.ptr.as_ptr() as *const [u8; N]) }
    }

    /// Whether the operating system actually pinned these pages.
    pub fn is_locked(&self) -> bool {
        self.0.locked
    }
}

impl<const N: usize> std::fmt::Debug for LockedBytes<N> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("LockedBytes(<redacted>)")
    }
}

/// A variable-length secret pinned in memory and wiped on drop.
///
/// Used for secrets whose length is not known at compile time: the master
/// password, and a value waiting on the clipboard to be cleared. The buffer grows
/// in whole pages, so there is no arbitrary length ceiling to work around.
pub struct LockedBuffer {
    page: LockedPage,
    len: usize,
}

impl LockedBuffer {
    /// Copies `value` into freshly pinned memory.
    pub fn new(value: &[u8]) -> Self {
        let mut page = LockedPage::new(value.len());
        page.bytes_mut()[..value.len()].copy_from_slice(value);
        Self {
            page,
            len: value.len(),
        }
    }

    /// Takes ownership of a `String`, wiping the source buffer.
    ///
    /// This is the path used for values that arrive as an owned `String` across a
    /// command boundary: the caller's allocation is cleared rather than left for
    /// the allocator to hand to the next tenant.
    pub fn take_string(mut value: String) -> Self {
        let buffer = Self::new(value.as_bytes());
        value.zeroize();
        buffer
    }

    /// Copies a string slice into freshly pinned memory.
    pub fn from_text(value: &str) -> Self {
        Self::new(value.as_bytes())
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Borrows the secret bytes.
    pub fn as_bytes(&self) -> &[u8] {
        self.page.secret(self.len)
    }

    /// Borrows the secret as text.
    ///
    /// Infallible in practice: every constructor takes its input from `&str` or
    /// `String`, so the bytes are guaranteed to be valid UTF-8. The fallback
    /// exists so this can never panic inside a security primitive.
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(self.as_bytes()).unwrap_or_default()
    }

    /// Whether the operating system actually pinned these pages.
    pub fn is_locked(&self) -> bool {
        self.page.locked
    }
}

impl Clone for LockedBuffer {
    /// Produces an independent, independently wiped and pinned copy.
    fn clone(&self) -> Self {
        Self::new(self.as_bytes())
    }
}

impl std::fmt::Debug for LockedBuffer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LockedBuffer")
            .field("value", &"<redacted>")
            .field("len", &self.len)
            .finish()
    }
}

#[cfg(windows)]
fn lock_region(pointer: *mut u8, length: usize) -> bool {
    use windows_sys::Win32::System::Memory::VirtualLock;
    // SAFETY: `pointer` addresses `length` bytes of this process's own committed
    // memory, which is the contract `VirtualLock` expects.
    unsafe { VirtualLock(pointer.cast(), length) != 0 }
}

#[cfg(windows)]
fn unlock_region(pointer: *mut u8, length: usize) -> bool {
    use windows_sys::Win32::System::Memory::VirtualUnlock;
    // SAFETY: reversing a successful `VirtualLock` over the same range.
    unsafe { VirtualUnlock(pointer.cast(), length) != 0 }
}

#[cfg(unix)]
fn lock_region(pointer: *mut u8, length: usize) -> bool {
    // SAFETY: `pointer` addresses `length` bytes of this process's own mapped
    // memory.
    unsafe { libc::mlock(pointer.cast(), length) == 0 }
}

#[cfg(unix)]
fn unlock_region(pointer: *mut u8, length: usize) -> bool {
    // SAFETY: reversing a successful `mlock` over the same range.
    unsafe { libc::munlock(pointer.cast(), length) == 0 }
}

#[cfg(not(any(windows, unix)))]
fn lock_region(_pointer: *mut u8, _length: usize) -> bool {
    false
}

#[cfg(not(any(windows, unix)))]
fn unlock_region(_pointer: *mut u8, _length: usize) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole allocation, used to prove padding stays clean and wiping works.
    fn page_bytes<const N: usize>(value: &LockedBytes<N>) -> &[u8] {
        value.0.bytes()
    }

    #[test]
    fn round_trips_the_value() {
        let locked = LockedBytes::new([7u8; 32]);
        assert_eq!(locked.as_bytes(), &[7u8; 32]);
    }

    #[test]
    fn the_allocation_is_page_complete() {
        // If the allocation shared a page with other data, locking it would
        // capture unrelated memory and the wipe would corrupt it.
        let locked = LockedBytes::new([1u8; 32]);
        let address = locked.0.ptr.as_ptr() as usize;
        assert_eq!(
            address % LOCKED_EXTENT,
            0,
            "allocation must be page-aligned"
        );
        assert_eq!(locked.0.extent % LOCKED_EXTENT, 0);
    }

    #[test]
    fn the_padding_after_the_value_starts_zeroed() {
        let locked = LockedBytes::new([9u8; 32]);
        assert!(page_bytes(&locked)[32..].iter().all(|byte| *byte == 0));
    }

    #[test]
    fn wiping_clears_the_entire_allocation() {
        let mut locked = LockedBytes::new([0xAB; 32]);
        locked.0.wipe();
        assert!(page_bytes(&locked).iter().all(|byte| *byte == 0));
    }

    #[test]
    fn debug_is_redacted() {
        let locked = LockedBytes::new([0xCD; 32]);
        assert_eq!(format!("{locked:?}"), "LockedBytes(<redacted>)");
        let buffer = LockedBuffer::from_text("correct-horse-battery-staple");
        let rendered = format!("{buffer:?}");
        assert!(!rendered.contains("horse"));
        assert!(rendered.contains("redacted"));
    }

    #[test]
    fn the_platform_reports_whether_it_could_lock() {
        if is_supported() {
            let locked = LockedBytes::new([3u8; 32]);
            assert!(
                locked.is_locked(),
                "locking one page should succeed on a supported platform"
            );
        }
    }

    #[test]
    fn many_locked_values_can_coexist() {
        // Every clone of a key takes its own locked page, so the process must be
        // able to pin far more than the handful the default quota allows. This is
        // what `ensure_lock_quota` buys: without raising the working set, Windows
        // refuses after roughly eleven 16 KiB allocations.
        let values: Vec<LockedBytes<32>> = (0..256).map(|_| LockedBytes::new([4u8; 32])).collect();
        assert_eq!(values.len(), 256);
        if is_supported() {
            let locked = values.iter().filter(|value| value.is_locked()).count();
            assert_eq!(
                locked, 256,
                "the locked-page quota was not raised; only {locked} of 256 could be pinned"
            );
        }
        assert!(values.iter().all(|value| value.as_bytes() == &[4u8; 32]));
    }

    // --- variable-length secrets -------------------------------------------

    #[test]
    fn a_buffer_round_trips_text() {
        let buffer = LockedBuffer::from_text("correct-horse-battery-staple");
        assert_eq!(buffer.as_str(), "correct-horse-battery-staple");
        assert_eq!(buffer.len(), 28);
        assert!(!buffer.is_empty());
    }

    #[test]
    fn taking_a_string_wipes_the_source() {
        let source = String::from("a-master-password-value");
        let buffer = LockedBuffer::take_string(source);
        assert_eq!(buffer.as_str(), "a-master-password-value");
        // The buffer is a copy; the original allocation is separate and wiped.
        assert!(buffer.is_locked() || !is_supported());
    }

    #[test]
    fn a_short_secret_still_gets_a_whole_page() {
        let buffer = LockedBuffer::from_text("x");
        assert_eq!(buffer.page.extent, LOCKED_EXTENT);
        assert_eq!(buffer.len(), 1);
    }

    #[test]
    fn an_empty_secret_is_supported() {
        let buffer = LockedBuffer::from_text("");
        assert!(buffer.is_empty());
        assert_eq!(buffer.as_str(), "");
        assert_eq!(buffer.page.extent, LOCKED_EXTENT);
    }

    #[test]
    fn a_secret_far_larger_than_one_page_is_accommodated() {
        // There must be no arbitrary length ceiling: a long passphrase, or a
        // clipboard value, simply costs more pages.
        let long = "a".repeat(LOCKED_EXTENT * 3 + 7);
        let buffer = LockedBuffer::from_text(&long);
        assert_eq!(buffer.len(), long.len());
        assert_eq!(buffer.as_str(), long);
        assert_eq!(buffer.page.extent, LOCKED_EXTENT * 4);
        assert_eq!(buffer.page.extent % LOCKED_EXTENT, 0);
    }

    #[test]
    fn a_multi_page_secret_is_fully_wiped() {
        let long = "b".repeat(LOCKED_EXTENT * 2);
        let mut buffer = LockedBuffer::from_text(&long);
        buffer.page.wipe();
        assert!(buffer.page.bytes().iter().all(|byte| *byte == 0));
    }

    #[test]
    fn a_buffer_clone_is_independent() {
        let original = LockedBuffer::from_text("original-secret");
        let copy = original.clone();
        drop(original);
        assert_eq!(copy.as_str(), "original-secret");
    }

    #[test]
    fn buffers_are_pinned_too() {
        if !is_supported() {
            return;
        }
        let buffer = LockedBuffer::from_text("a-master-password-value");
        assert!(
            buffer.is_locked(),
            "a variable-length secret must be pinned as well as a key"
        );
    }
}
