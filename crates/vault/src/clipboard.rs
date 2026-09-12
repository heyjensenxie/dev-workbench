//! Secure clipboard handling.
//!
//! Copying a password necessarily hands it to a system-wide resource that other
//! applications can read. That is a limitation of the operating system, not
//! something this module can fix, and the UI says so plainly rather than
//! implying the clipboard is private.
//!
//! What this module *can* do is bound how long the exposure lasts, and avoid
//! destroying data that is no longer ours:
//!
//! * [`write_text`] places a value on the clipboard.
//! * [`clear_if_unchanged`] removes it again **only if the clipboard still holds
//!   exactly that value**. If another application copied something in the
//!   meantime, that content is left alone — a naive timed clear would silently
//!   destroy the user's unrelated clipboard contents.
//!
//! The staging buffers used while writing are wiped after use.

use zeroize::Zeroizing;

use crate::error::VaultResult;

/// Whether this platform has a native implementation available.
pub const fn is_supported() -> bool {
    cfg!(windows)
}

#[cfg(windows)]
mod platform {
    use super::*;
    use crate::error::VaultError;
    use std::ptr;
    use std::slice;
    use std::time::Duration;
    use zeroize::Zeroize;
    use windows_sys::Win32::Foundation::HGLOBAL;
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable,
        OpenClipboard, SetClipboardData,
    };
    use windows_sys::Win32::System::Memory::{
        GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock,
    };

    /// `CF_UNICODETEXT` is a stable Win32 ABI constant (13). `windows-sys`
    /// exposes it only through the OLE/COM module, which this crate does not
    /// otherwise need, so it is declared here with its documented value.
    const CF_UNICODETEXT: u32 = 13;

    // `GlobalFree` is not exposed by the `windows-sys` build in use, but is
    // required to release a clipboard allocation when the system does *not*
    // take ownership (i.e. when `SetClipboardData` fails).
    unsafe extern "system" {
        fn GlobalFree(hmem: HGLOBAL) -> HGLOBAL;
    }

    /// Opens the clipboard, retrying briefly because another process may hold it.
    fn open_clipboard() -> bool {
        for _ in 0..5 {
            // SAFETY: passing a null owner window is the documented way to open
            // the clipboard without associating it with a window.
            if unsafe { OpenClipboard(ptr::null_mut()) } != 0 {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }

    /// Reads the clipboard as UTF-16 text, returning a self-wiping buffer.
    pub fn read_text() -> Option<Zeroizing<String>> {
        if !open_clipboard() {
            return None;
        }
        let text = read_text_locked();
        // SAFETY: the clipboard was opened immediately above and is released
        // here on every path, including the early returns inside the closure.
        unsafe { CloseClipboard() };
        text
    }

    fn read_text_locked() -> Option<Zeroizing<String>> {
        // SAFETY: the clipboard is held open by the caller for the duration of
        // this function.
        unsafe {
            if IsClipboardFormatAvailable(CF_UNICODETEXT) == 0 {
                return None;
            }
            let handle = GetClipboardData(CF_UNICODETEXT);
            if handle.is_null() {
                return None;
            }
            // The block stays owned by the clipboard; we may only lock it.
            let pointer = GlobalLock(handle) as *const u16;
            if pointer.is_null() {
                return None;
            }
            let units = GlobalSize(handle) / std::mem::size_of::<u16>();
            let mut length = 0usize;
            while length < units && *pointer.add(length) != 0 {
                length += 1;
            }
            let decoded = String::from_utf16_lossy(slice::from_raw_parts(pointer, length));
            let result = Zeroizing::new(decoded);
            GlobalUnlock(handle);
            Some(result)
        }
    }

    /// Replaces the clipboard contents with `value`.
    pub fn write_text(value: &str) -> VaultResult<()> {
        let mut wide: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
        let byte_len = wide.len() * std::mem::size_of::<u16>();

        // SAFETY: every pointer is checked before use, and ownership of the
        // allocation is either transferred to the clipboard or freed here.
        unsafe {
            let handle = GlobalAlloc(GMEM_MOVEABLE, byte_len);
            if handle.is_null() {
                wide.zeroize();
                return Err(VaultError::Crypto("clipboard allocation failed"));
            }
            let destination = GlobalLock(handle) as *mut u16;
            if destination.is_null() {
                GlobalFree(handle);
                wide.zeroize();
                return Err(VaultError::Crypto("clipboard lock failed"));
            }
            ptr::copy_nonoverlapping(wide.as_ptr(), destination, wide.len());
            GlobalUnlock(handle);
            // The staging copy of the secret is no longer needed.
            wide.zeroize();

            if !open_clipboard() {
                GlobalFree(handle);
                return Err(VaultError::Crypto("clipboard is busy"));
            }
            if EmptyClipboard() == 0 {
                CloseClipboard();
                GlobalFree(handle);
                return Err(VaultError::Crypto("clipboard could not be cleared"));
            }
            let taken = SetClipboardData(CF_UNICODETEXT, handle);
            CloseClipboard();
            if taken.is_null() {
                // The system did not take ownership, so the block is still ours.
                GlobalFree(handle);
                return Err(VaultError::Crypto("clipboard write failed"));
            }
        }
        Ok(())
    }

    /// Empties the clipboard unconditionally.
    pub fn clear() -> VaultResult<()> {
        // SAFETY: the clipboard is opened before being emptied and released
        // afterwards; no pointer is derived from user input.
        unsafe {
            if !open_clipboard() {
                return Err(VaultError::Crypto("clipboard is busy"));
            }
            let emptied = EmptyClipboard();
            CloseClipboard();
            if emptied == 0 {
                return Err(VaultError::Crypto("clipboard could not be cleared"));
            }
        }
        Ok(())
    }
}

#[cfg(not(windows))]
mod platform {
    use super::*;
    use crate::error::VaultError;

    pub fn read_text() -> Option<Zeroizing<String>> {
        None
    }

    pub fn write_text(_value: &str) -> VaultResult<()> {
        Err(VaultError::ClipboardUnsupported)
    }

    pub fn clear() -> VaultResult<()> {
        Err(VaultError::ClipboardUnsupported)
    }
}

pub use platform::{clear, read_text, write_text};

/// Removes the clipboard contents only when they are still the value we wrote.
///
/// Returns `true` when the clipboard was cleared, `false` when it had already
/// been replaced by something else.
pub fn clear_if_unchanged(expected: &str) -> VaultResult<bool> {
    let Some(current) = read_text() else {
        return Ok(false);
    };
    if current.as_str() != expected {
        return Ok(false);
    }
    clear()?;
    Ok(true)
}
