//! OS keychain boundary. Secrets must never be persisted in SQLite.

#[cfg(windows)]
use std::{ptr, slice};

#[cfg(windows)]
use windows_sys::Win32::Security::Credentials::{
    CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW, CredDeleteW, CredFree, CredReadW,
    CredWriteW,
};

pub type SecretResult<T> = Result<T, String>;

#[cfg(windows)]
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Stores a secret in the current user's Windows Credential Manager.
#[cfg(windows)]
pub fn write(target: &str, username: &str, secret: &str) -> SecretResult<()> {
    let target = wide(target);
    let username = wide(username);
    let mut blob = secret.as_bytes().to_vec();
    let credential = CREDENTIALW {
        Type: CRED_TYPE_GENERIC,
        TargetName: target.as_ptr() as *mut u16,
        CredentialBlobSize: blob.len() as u32,
        CredentialBlob: blob.as_mut_ptr(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        UserName: username.as_ptr() as *mut u16,
        ..Default::default()
    };
    let result = unsafe { CredWriteW(&credential, 0) };
    blob.fill(0);
    if result == 0 {
        Err(std::io::Error::last_os_error().to_string())
    } else {
        Ok(())
    }
}

/// Reads a secret from the current user's Windows Credential Manager.
#[cfg(windows)]
pub fn read(target: &str) -> SecretResult<Option<String>> {
    let target = wide(target);
    let mut raw: *mut CREDENTIALW = ptr::null_mut();
    let result = unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut raw) };
    if result == 0 {
        return Ok(None);
    }
    let credential = unsafe { &*raw };
    let bytes = unsafe {
        slice::from_raw_parts(
            credential.CredentialBlob,
            credential.CredentialBlobSize as usize,
        )
    };
    let secret_bytes = bytes.to_vec();
    unsafe { CredFree(raw.cast()) };
    let secret = String::from_utf8(secret_bytes)
        .map_err(|_| "stored credential is not valid UTF-8".to_owned())?;
    Ok(Some(secret))
}

/// Removes a secret from the current user's Windows Credential Manager.
#[cfg(windows)]
pub fn delete(target: &str) -> SecretResult<()> {
    let target = wide(target);
    let result = unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) };
    if result == 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(1168) {
            Ok(())
        } else {
            Err(error.to_string())
        }
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
pub fn write(_target: &str, _username: &str, _secret: &str) -> SecretResult<()> {
    Err("OS secret storage is not implemented on this platform".into())
}

#[cfg(not(windows))]
pub fn read(_target: &str) -> SecretResult<Option<String>> {
    Err("OS secret storage is not implemented on this platform".into())
}

#[cfg(not(windows))]
pub fn delete(_target: &str) -> SecretResult<()> {
    Err("OS secret storage is not implemented on this platform".into())
}
