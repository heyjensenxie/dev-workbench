//! Tauri command surface for the Password Vault.
//!
//! This module is the vault's *only* integration point with the rest of Dev
//! Workbench, and it is deliberately thin. Everything it exposes maps onto one
//! narrow method of [`VaultService`]; no cryptographic material, no vault
//! master key, and no raw database handle crosses this boundary.
//!
//! Two rules are enforced here rather than trusted to callers:
//!
//! * **The master password is used and dropped.** It arrives as a `String`,
//!   is moved straight into [`MasterPassword`] (whose backing buffer is wiped on
//!   drop), and is never stored, logged, echoed back, or placed on the
//!   clipboard. Nothing about it appears in a command's return value.
//! * **Returned data is the minimum the screen needs.** Status, item summaries,
//!   and one full item at a time. The list never carries passwords.
//!
//! The command set deliberately contains no `getPassword`, no `exportPlaintext`,
//! and no `listSecrets`. Only `vault_open` and `vault_lock` are registered as
//! generic workbench commands (see `packages/core`), because the command
//! registry is reachable from the palette, from plugins, and eventually from AI
//! actions.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, State};
use workbench_vault::locked::LockedBuffer;
use workbench_vault::service::generator_options_from_value;
use workbench_vault::{MasterPassword, VaultBackupSummary, VaultItem, VaultItemPayload, VaultItemSummary, VaultService, VaultStatus};

use crate::error::AppError;

/// How often the native layer re-checks idle expiry and resume-from-sleep.
const AUTO_LOCK_TICK: Duration = Duration::from_secs(5);

/// Longest clipboard clear delay accepted from the UI.
const MAX_CLIPBOARD_CLEAR_SECONDS: u64 = 600;

/// Reason string emitted with every `vault-locked` event.
pub mod lock_reason {
    pub const MANUAL: &str = "manual";
    pub const IDLE_TIMEOUT: &str = "idle-timeout";
    pub const RESUMED_FROM_SLEEP: &str = "resumed-from-sleep";
    pub const DESTROYED: &str = "destroyed";
}

/// Owns the vault service plus the clipboard-clear bookkeeping.
pub struct VaultState {
    service: Arc<VaultService>,
    /// The application data directory, so a pre-replace safety copy lands beside
    /// the vault rather than wherever the process happens to be.
    data_dir: PathBuf,
    /// Generation counter for clipboard timers. A newer copy invalidates any
    /// timer still pending for an older one.
    clipboard_generation: AtomicU64,
}

impl VaultState {
    pub fn new(service: VaultService, data_dir: PathBuf) -> Self {
        Self {
            service: Arc::new(service),
            data_dir,
            clipboard_generation: AtomicU64::new(0),
        }
    }
}

/// Result of restoring a backup.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultImportOutcome {
    pub item_count: usize,
    pub format_version: u32,
    /// Where the previous vault was saved before being replaced, when there was
    /// one and a replace was requested.
    pub safety_backup: Option<String>,
}

/// Emits `vault-locked` so every window can drop its in-memory copy of the data.
fn announce_lock(app: &AppHandle, reason: &'static str) {
    let _ = app.emit("vault-locked", reason);
}

// ---------------------------------------------------------------- lifecycle

#[tauri::command]
pub async fn vault_status(state: State<'_, VaultState>) -> Result<VaultStatus, AppError> {
    Ok(state.service.status().await?)
}

#[tauri::command]
pub async fn vault_create(
    master_password: String,
    state: State<'_, VaultState>,
) -> Result<VaultStatus, AppError> {
    // The password is moved into a self-wiping wrapper immediately.
    let password = MasterPassword::new(master_password);
    Ok(state.service.create(password).await?)
}

#[tauri::command]
pub async fn vault_unlock(
    master_password: String,
    state: State<'_, VaultState>,
) -> Result<VaultStatus, AppError> {
    let password = MasterPassword::new(master_password);
    Ok(state.service.unlock(password).await?)
}

#[tauri::command]
pub async fn vault_lock(
    app: AppHandle,
    state: State<'_, VaultState>,
) -> Result<bool, AppError> {
    let locked = state.service.lock().await;
    if locked {
        announce_lock(&app, lock_reason::MANUAL);
    }
    Ok(locked)
}

/// Extends the idle deadline. The UI calls this on real user activity only.
#[tauri::command]
pub async fn vault_touch(state: State<'_, VaultState>) -> Result<(), AppError> {
    state.service.touch();
    Ok(())
}

#[tauri::command]
pub async fn vault_set_auto_lock(
    seconds: u64,
    state: State<'_, VaultState>,
) -> Result<(), AppError> {
    state.service.set_auto_lock_seconds(seconds);
    Ok(())
}

// -------------------------------------------------------------------- items

#[tauri::command]
pub async fn vault_list_items(
    state: State<'_, VaultState>,
) -> Result<Vec<VaultItemSummary>, AppError> {
    Ok(state.service.list_items().await?)
}

#[tauri::command]
pub async fn vault_get_item(
    id: String,
    state: State<'_, VaultState>,
) -> Result<Option<VaultItem>, AppError> {
    Ok(state.service.get_item(&id).await?)
}

#[tauri::command]
pub async fn vault_create_item(
    payload: VaultItemPayload,
    state: State<'_, VaultState>,
) -> Result<VaultItem, AppError> {
    Ok(state.service.create_item(payload).await?)
}

#[tauri::command]
pub async fn vault_update_item(
    id: String,
    payload: VaultItemPayload,
    state: State<'_, VaultState>,
) -> Result<VaultItem, AppError> {
    Ok(state.service.update_item(&id, payload).await?)
}

#[tauri::command]
pub async fn vault_delete_item(id: String, state: State<'_, VaultState>) -> Result<bool, AppError> {
    Ok(state.service.delete_item(&id).await?)
}

/// Removes the vault from this machine.
///
/// Deliberately unauthenticated: see [`workbench_vault::VaultService::destroy`]
/// for why a master-password prompt here would be theatre rather than protection.
/// The user-facing guard is a typed confirmation in the UI.
#[tauri::command]
pub async fn vault_destroy(
    app: AppHandle,
    state: State<'_, VaultState>,
) -> Result<(), AppError> {
    state.service.destroy().await?;
    // Nothing of the vault should outlive it, including a copied password.
    let _ = workbench_vault::clipboard::clear();
    announce_lock(&app, lock_reason::DESTROYED);
    Ok(())
}

/// Toggles an item's favourite flag natively.
///
/// The flag is inside the encrypted payload, so this decrypts and re-encrypts the
/// record in Rust. That matters now that items are redacted: the previous
/// approach — fetch the item, flip a boolean, send it back — would have wiped the
/// stored password.
#[tauri::command]
pub async fn vault_set_favorite(
    id: String,
    favorite: bool,
    state: State<'_, VaultState>,
) -> Result<VaultItem, AppError> {
    Ok(state.service.set_favorite(&id, favorite).await?)
}

/// Reveals exactly one secret field, on explicit request.
///
/// `field_index` of `null` selects the item's password; a number selects a
/// sensitive custom field. This is the **only** way a secret leaves the vault, and
/// it returns one field at a time. Opening an item does not do this.
#[tauri::command]
pub async fn vault_reveal_field(
    id: String,
    field_index: Option<usize>,
    state: State<'_, VaultState>,
) -> Result<Option<String>, AppError> {
    let value = state.service.reveal_field(&id, field_index).await?;
    Ok(value.map(|value| value.as_str().to_owned()))
}

/// Copies one secret field straight to the clipboard.
///
/// The value is decrypted in Rust and written to the clipboard here; it is never
/// returned to the caller, so the copied password does not enter the WebView at
/// all. Copying is the common case, and it no longer requires the plaintext to be
/// rendered anywhere.
#[tauri::command]
pub async fn vault_copy_item_field(
    id: String,
    field_index: Option<usize>,
    clear_after_seconds: u64,
    app: AppHandle,
    state: State<'_, VaultState>,
) -> Result<(), AppError> {
    let value = state
        .service
        .reveal_field(&id, field_index)
        .await?
        .ok_or_else(|| AppError::NotFound("that field is not stored for this item".into()))?;
    place_on_clipboard(&app, state.inner(), value, clear_after_seconds)
}

#[tauri::command]
pub async fn vault_change_master_password(
    current_password: String,
    new_password: String,
    state: State<'_, VaultState>,
) -> Result<(), AppError> {
    let current = MasterPassword::new(current_password);
    let new = MasterPassword::new(new_password);
    Ok(state.service.change_master_password(&current, &new).await?)
}

/// Re-benchmarks the key-derivation cost and re-wraps the vault key.
///
/// Touches no record: only the small wrapper around the vault master key is
/// rewritten, so this is quick regardless of how many items the vault holds.
#[tauri::command]
pub async fn vault_recalibrate(
    master_password: String,
    state: State<'_, VaultState>,
) -> Result<VaultStatus, AppError> {
    let password = MasterPassword::new(master_password);
    Ok(state.service.recalibrate(&password).await?)
}

// ------------------------------------------------------------------- backup

#[tauri::command]
pub async fn vault_export_backup(
    path: String,
    state: State<'_, VaultState>,
) -> Result<VaultBackupSummary, AppError> {
    let document = state.service.export_backup().await?;
    let target = backup_path(&path);
    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(&target, document.as_bytes()).await?;
    let meta = state.service.status().await?;
    Ok(VaultBackupSummary {
        item_count: meta.item_count as usize,
        format_version: meta.format_version,
    })
}

/// Restores a backup, optionally replacing the vault that is already here.
///
/// When `replace_existing` is true and a vault exists, the current vault is first
/// written to a timestamped safety copy under the application data directory, and
/// its path is returned. Replacing a vault is otherwise irreversible, and the most
/// likely mistake — restoring a stale backup over a current one — would silently
/// discard recent credentials. The safety copy makes that recoverable.
///
/// Refusing by default is what keeps a stray import from destroying live
/// credentials; the safety copy is what makes the deliberate replace safe to offer.
#[tauri::command]
pub async fn vault_import_backup(
    path: String,
    replace_existing: bool,
    state: State<'_, VaultState>,
) -> Result<VaultImportOutcome, AppError> {
    let document = tokio::fs::read_to_string(backup_path(&path)).await?;

    // Take the safety copy before the replace, so the previous vault survives even
    // if the import itself fails partway through.
    let mut safety_backup = None;
    if replace_existing && state.service.status().await?.exists {
        let document = state.service.export_backup().await?;
        let target = safety_backup_path(&state.data_dir);
        if let Some(parent) = target.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(&target, document.as_bytes()).await?;
        safety_backup = Some(target.to_string_lossy().into_owned());
    }

    let summary = state.service.import_backup(&document, replace_existing).await?;
    Ok(VaultImportOutcome {
        item_count: summary.item_count,
        format_version: summary.format_version,
        safety_backup,
    })
}

/// Where a pre-replace safety copy is written.
///
/// A timestamped name per replace, rather than a single overwritten file: the
/// whole point is that a previous state survives a later mistake, and "the one
/// safety copy" would be clobbered by the next replace.
fn safety_backup_path(data_dir: &Path) -> PathBuf {
    data_dir
        .join("backups")
        .join(format!("vault-before-restore-{}.vaultbackup", crate::models::now_millis()))
}

/// Normalises a backup destination to the vault's own extension. Never writes a
/// `.csv`, `.json`, or other plaintext-friendly extension by accident.
fn backup_path(path: &str) -> PathBuf {
    let candidate = PathBuf::from(path.trim());
    match candidate.extension().and_then(|value| value.to_str()) {
        Some("vaultbackup") => candidate,
        _ => {
            let mut with_extension = candidate.into_os_string();
            with_extension.push(".vaultbackup");
            PathBuf::from(with_extension)
        }
    }
}

// ---------------------------------------------------------------- generator

/// Returns a freshly generated password.
///
/// This is not a secret *read*: the value was just created, is not stored
/// anywhere, and is not logged. The UI shows it and offers to apply it.
#[tauri::command]
pub async fn vault_generate_password(
    options: Option<Value>,
    state: State<'_, VaultState>,
) -> Result<String, AppError> {
    use workbench_vault::secrecy::ExposeSecret;
    let options = generator_options_from_value(options.unwrap_or(Value::Null))?;
    let generated = state.service.generate_password(&options)?;
    Ok(generated.expose_secret().to_owned())
}

// ---------------------------------------------------------------- clipboard

/// Writes a secret to the clipboard and schedules its removal.
///
/// The only place a clipboard value is written, so every caller gets the same
/// guarantees: the value is held in locked memory for its whole lifetime, and the
/// scheduled clear fires only if the clipboard still holds exactly that value —
/// someone who copies something else in the meantime keeps it.
///
/// A `clear_after_seconds` of `0` means "do not schedule a clear", which the UI
/// uses for non-secret values such as a username.
fn place_on_clipboard(
    app: &AppHandle,
    state: &VaultState,
    value: LockedBuffer,
    clear_after_seconds: u64,
) -> Result<(), AppError> {
    workbench_vault::clipboard::write_text(value.as_str())?;

    if clear_after_seconds == 0 {
        return Ok(());
    }
    let seconds = clear_after_seconds.min(MAX_CLIPBOARD_CLEAR_SECONDS);
    let generation = state.clipboard_generation.fetch_add(1, Ordering::SeqCst) + 1;
    let app = app.clone();

    // The value lives in this task for up to a minute — long enough that leaving
    // it pageable would be a real exposure — so it stays pinned until the timer
    // fires.
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(seconds)).await;

        // A newer copy superseded this timer; leave that value alone.
        let Some(state) = app.try_state::<VaultState>() else {
            return;
        };
        if state.clipboard_generation.load(Ordering::SeqCst) != generation {
            return;
        }
        // Best-effort: the clipboard may be locked by another process, and the
        // user may have replaced its contents, in which case nothing is cleared.
        let _ = workbench_vault::clipboard::clear_if_unchanged(value.as_str());
    });
    Ok(())
}

/// Places a non-secret value on the system clipboard.
///
/// Used for usernames and URLs. Passwords go through
/// [`vault_copy_item_field`], which never hands the plaintext to the caller.
#[tauri::command]
pub async fn vault_copy_secret(
    value: String,
    clear_after_seconds: u64,
    app: AppHandle,
    state: State<'_, VaultState>,
) -> Result<(), AppError> {
    place_on_clipboard(
        &app,
        state.inner(),
        LockedBuffer::take_string(value),
        clear_after_seconds,
    )
}

/// Empties the clipboard immediately — used when the vault locks.
///
/// Advancing the generation also neutralises any pending timer, so a later
/// clear cannot race with whatever the user copies next.
#[tauri::command]
pub async fn vault_clear_clipboard(state: State<'_, VaultState>) -> Result<(), AppError> {
    state.clipboard_generation.fetch_add(1, Ordering::SeqCst);
    workbench_vault::clipboard::clear()?;
    Ok(())
}

// ----------------------------------------------------------------- open URL

/// Opens an item's URL in the default browser.
///
/// Only `http` and `https` are accepted. Every other scheme — `javascript:`,
/// `file:`, `shell:`, `powershell:`, `cmd:`, `data:`, `vbscript:`, `ms-msdt:`,
/// and anything unrecognised — is rejected, so a stored URL can never become a
/// command-execution or local-file primitive.
#[tauri::command]
pub async fn vault_open_url(url: String) -> Result<(), AppError> {
    let url = url.trim();
    if url.chars().any(|character| character.is_control()) {
        return Err(AppError::Validation(
            "URL must not contain control characters".into(),
        ));
    }
    match scheme_of(url) {
        Some(scheme) if scheme.eq_ignore_ascii_case("https") || scheme.eq_ignore_ascii_case("http") => {}
        Some(scheme) => {
            return Err(AppError::Validation(format!(
                "refusing to open a '{scheme}' URL; only http and https are allowed"
            )));
        }
        None => {
            return Err(AppError::Validation(
                "URL must start with http:// or https://".into(),
            ));
        }
    }

    #[cfg(target_os = "windows")]
    let mut command = std::process::Command::new("explorer");
    #[cfg(target_os = "macos")]
    let mut command = std::process::Command::new("open");
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = std::process::Command::new("xdg-open");
    // Arguments are passed as a vector, never through a shell, so the URL cannot
    // be interpreted as a command.
    command.arg(url);
    command.spawn()?;
    Ok(())
}

/// Extracts the scheme of a URL, or `None` when it does not look like one.
fn scheme_of(url: &str) -> Option<&str> {
    // A Windows drive path has a colon but is not a URL: what precedes it is a
    // single letter followed by a path separator, not a scheme.
    let bytes = url.as_bytes();
    if bytes.len() > 2 && bytes[1] == b':' && matches!(bytes[2], b'\\' | b'/') {
        return None;
    }
    let colon = url.find(':')?;
    let scheme = &url[..colon];
    if scheme.is_empty() {
        return None;
    }
    let valid = scheme
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '.'));
    // A scheme must start with a letter.
    if !valid || !scheme.starts_with(|character: char| character.is_ascii_alphabetic()) {
        return None;
    }
    Some(scheme)
}

// -------------------------------------------------------------- auto locking

/// Starts the background supervisors that lock the vault without user action.
///
/// Two independent conditions are watched:
///
/// * the idle timeout, evaluated natively so it still fires if the WebView is
///   busy, frozen, or closed;
/// * a resume after the machine was suspended, detected by comparing wall-clock
///   progress against monotonic progress. A monotonic clock does not advance
///   while a machine sleeps, so without this check a closed laptop would come
///   back still unlocked.
pub fn spawn_auto_lock_supervisor(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut ticker = tokio::time::interval(AUTO_LOCK_TICK);
        loop {
            ticker.tick().await;
            let service = {
                let Some(state) = app.try_state::<VaultState>() else {
                    continue;
                };
                state.service.clone()
            };

            if service.session().detect_resume_from_sleep() {
                if service.lock().await {
                    let _ = workbench_vault::clipboard::clear();
                    announce_lock(&app, lock_reason::RESUMED_FROM_SLEEP);
                }
                continue;
            }
            if service.sweep_auto_lock().await {
                // The clipboard may still be holding a password copied moments
                // before the lock; drop it rather than leaving it behind.
                let _ = workbench_vault::clipboard::clear();
                announce_lock(&app, lock_reason::IDLE_TIMEOUT);
            }
        }
    });
}

/// Locks the vault because the application is shutting down.
pub fn lock_on_exit(state: &VaultState) {
    let service = state.service.clone();
    let _ = tauri::async_runtime::block_on(service.lock());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_http_and_https() {
        assert_eq!(scheme_of("https://github.com"), Some("https"));
        assert_eq!(scheme_of("http://localhost:3000/x"), Some("http"));
        assert_eq!(scheme_of("HTTPS://example.com"), Some("HTTPS"));
    }

    #[test]
    fn extracts_dangerous_schemes_so_they_can_be_refused() {
        assert_eq!(scheme_of("javascript:alert(1)"), Some("javascript"));
        assert_eq!(scheme_of("file:///C:/Windows"), Some("file"));
        assert_eq!(scheme_of("powershell:-Command x"), Some("powershell"));
        assert_eq!(scheme_of("cmd:/c whoami"), Some("cmd"));
        assert_eq!(scheme_of("data:text/html,<script>"), Some("data"));
    }

    #[test]
    fn rejects_things_that_are_not_urls_at_all() {
        assert_eq!(scheme_of("github.com"), None);
        assert_eq!(scheme_of(""), None);
        // A Windows drive path is not a URL scheme: it starts with a drive
        // letter but the "scheme" contains a path separator.
        assert_eq!(scheme_of("C:\\Windows\\System32"), None);
        assert_eq!(scheme_of("://missing-scheme"), None);
    }

    #[test]
    fn always_writes_the_vault_backup_extension() {
        assert_eq!(
            backup_path("C:/backups/vault"),
            PathBuf::from("C:/backups/vault.vaultbackup")
        );
        assert_eq!(
            backup_path("C:/backups/vault.vaultbackup"),
            PathBuf::from("C:/backups/vault.vaultbackup")
        );
        // A plaintext-friendly extension is never preserved.
        assert_eq!(
            backup_path("C:/backups/vault.json"),
            PathBuf::from("C:/backups/vault.json.vaultbackup")
        );
        assert_eq!(
            backup_path("C:/backups/vault.csv"),
            PathBuf::from("C:/backups/vault.csv.vaultbackup")
        );
    }
}
