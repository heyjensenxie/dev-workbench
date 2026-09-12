//! The vault service: the only supported way to touch vault data.
//!
//! This type is the security boundary's front door. Everything sensitive is
//! private to it:
//!
//! * callers never receive a [`crate::key::VaultKey`], a KEK, or a raw database
//!   handle;
//! * callers never choose the associated data or the nonce for a record;
//! * callers cannot decrypt an individual record themselves.
//!
//! What callers *can* do is deliberately narrow, and every method that touches
//! plaintext requires an unlocked vault:
//!
//! * [`VaultService::list_items`] returns a projection with no passwords, no
//!   notes, and no custom field values, so simply browsing the vault does not
//!   push every secret into the UI process;
//! * [`VaultService::get_item`] returns one full item, on demand, for as long as
//!   it is displayed;
//! * [`VaultService::create_item`], [`update_item`](Self::update_item) and
//!   [`delete_item`](Self::delete_item) mutate one record at a time.
//!
//! ## What is deliberately not claimed
//!
//! Deleting an item removes a *row*. It does not guarantee that the underlying
//! bytes are gone from the physical medium: SQLite, the filesystem's journal,
//! and above all SSD wear-levelling and over-provisioning all keep copies that
//! the application cannot reach. There is no "secure erase" here, and the UI
//! must not promise one.

use std::path::Path;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use zeroize::Zeroizing;

use crate::crypto;
use crate::error::{VaultError, VaultResult};
use crate::generator::GeneratorOptions;
use crate::kdf::{self, KdfParams};
use crate::key::{self, MasterPassword, VaultKey};
use crate::locked::LockedBuffer;
use crate::lockout;
use crate::model::{
    ITEM_RECORD_VERSION, VAULT_FORMAT_VERSION, VaultBackupSummary, VaultItem, VaultItemPayload,
    VaultItemSummary, VaultKdfSummary, VaultStatus,
};
use crate::session::VaultSession;
use crate::storage::{StoredRecord, VaultMeta, VaultStorage};

/// Marker written into every backup file, so a wrong file is rejected by
/// inspection rather than by a confusing parse error.
const BACKUP_MAGIC: &str = "dev-workbench.vault.backup";
/// Version of the backup envelope.
const BACKUP_FORMAT_VERSION: u32 = 1;

/// Associated data prefix binding a record's ciphertext to its row id and to the
/// vault format. A ciphertext therefore cannot be moved to another item, or
/// replayed into a vault built by a different format version.
const ITEM_AAD_PREFIX: &[u8] = b"dev-workbench.vault.v1.item:";

pub struct VaultService {
    storage: VaultStorage,
    session: VaultSession,
}

impl VaultService {
    /// Opens the vault database at `path`. Creates an empty file if absent; the
    /// vault itself is not initialised until [`create`](Self::create) runs.
    pub async fn open(path: &Path) -> VaultResult<Self> {
        Ok(Self {
            storage: VaultStorage::open(path).await?,
            session: VaultSession::new(),
        })
    }

    /// Borrows the session, for the auto-lock sweeper and clipboard timers.
    pub fn session(&self) -> &VaultSession {
        &self.session
    }

    // ---------------------------------------------------------------- status

    pub async fn status(&self) -> VaultResult<VaultStatus> {
        let meta = self.storage.load_meta().await?;
        let throttle = self.storage.load_throttle().await.unwrap_or_default();
        Ok(VaultStatus {
            exists: meta.is_some(),
            unlocked: self.session.is_unlocked().await,
            format_version: meta
                .as_ref()
                .map_or(VAULT_FORMAT_VERSION, |meta| meta.format_version),
            item_count: self.storage.count_records().await.unwrap_or(0),
            auto_lock_seconds: self.session.auto_lock_seconds(),
            memory_locked: self.session.is_key_memory_locked().await,
            failed_attempts: throttle.failed_attempts,
            locked_until: throttle.locked_until,
            kdf: meta.as_ref().map(|meta| VaultKdfSummary {
                algorithm: kdf::ALGORITHM_ARGON2ID,
                version: meta.kdf.version,
                memory_kib: meta.kdf.memory_kib,
                time_cost: meta.kdf.time_cost,
                parallelism: meta.kdf.parallelism,
                meets_current_floor: meta.kdf.meets_current_floor(),
            }),
        })
    }

    // ---------------------------------------------------------- key lifecycle

    /// Creates a new vault.
    ///
    /// Argon2id parameters are calibrated against this machine, a random salt is
    /// generated, a random 256-bit vault master key is generated, and that key
    /// is wrapped under the password-derived KEK. The master password itself is
    /// used only to derive the KEK and is never stored in any form.
    pub async fn create(&self, password: MasterPassword) -> VaultResult<VaultStatus> {
        let params = kdf::calibrate(kdf::DEFAULT_CALIBRATION_TARGET);
        self.create_with_params(password, params).await
    }

    /// Creates a vault with explicit KDF parameters.
    ///
    /// Exposed so the test-suite can avoid minutes of Argon2id work. Ordinary
    /// callers must use [`create`](Self::create), which calibrates for the
    /// machine and never picks a deliberately cheap configuration.
    #[doc(hidden)]
    pub async fn create_with_params(
        &self,
        password: MasterPassword,
        params: KdfParams,
    ) -> VaultResult<VaultStatus> {
        password.validate()?;
        if self.storage.is_initialized().await? {
            return Err(VaultError::AlreadyInitialized);
        }

        let kek = params.derive(&password)?;
        let vault_master_key = VaultKey::generate();
        let wrapped_key = key::wrap_key(&kek, &vault_master_key)?;

        let now = crate::now_millis();
        let meta = VaultMeta {
            format_version: VAULT_FORMAT_VERSION,
            kdf: params,
            wrapped_key,
            created_at: now,
            updated_at: now,
        };
        self.storage.insert_meta(&meta).await?;
        self.session.unlock(vault_master_key).await;
        self.status().await
    }

    /// Unlocks the vault, returning the uniform error on any failure.
    ///
    /// A wrong password, a corrupted header, and a failed authentication tag all
    /// surface as [`VaultError::IncorrectMasterPassword`]. Telling them apart
    /// would hand an attacker a free oracle.
    ///
    /// A lockout is checked *before* the key derivation, so a throttled caller
    /// cannot keep the CPU busy or make the user wait through a derivation that
    /// was never going to be attempted.
    pub async fn unlock(&self, password: MasterPassword) -> VaultResult<VaultStatus> {
        let meta = self
            .storage
            .load_meta()
            .await?
            .ok_or(VaultError::NotInitialized)?;
        self.refuse_while_locked_out().await?;

        match self.unwrap_with(&meta, &password)? {
            Some(key) => {
                // Only a correct password clears the throttle, so an expired
                // lockout still escalates the next time.
                self.storage.clear_throttle().await?;
                self.session.unlock(key).await;
                self.status().await
            }
            None => Err(self.register_failure().await),
        }
    }

    /// Refuses an attempt while a lockout is in force.
    async fn refuse_while_locked_out(&self) -> VaultResult<()> {
        let throttle = self.storage.load_throttle().await?;
        let Some(until) = throttle.locked_until else {
            return Ok(());
        };
        let remaining_ms = until.saturating_sub(crate::now_millis());
        if remaining_ms <= 0 {
            return Ok(());
        }
        // Round up, so a remaining fraction of a second never reports zero.
        let seconds = (remaining_ms as u64).div_ceil(1_000);
        Err(VaultError::LockedOut { seconds })
    }

    /// Counts a failed attempt, applies its delay, and returns the error to
    /// surface.
    ///
    /// Shared by every path that verifies the master password, so an attacker
    /// cannot side-step the throttle by going through a different command.
    async fn register_failure(&self) -> VaultError {
        let throttle = self.storage.load_throttle().await.unwrap_or_default();
        let failed_attempts = throttle.failed_attempts.saturating_add(1);
        let lockout = lockout::lockout_seconds(failed_attempts);
        let locked_until = if lockout > 0 {
            Some(crate::now_millis().saturating_add((lockout * 1_000) as i64))
        } else {
            None
        };
        // A failure to persist the throttle must not turn into a free attempt, but
        // there is nothing useful to do about it here either: the next attempt
        // reads the same header.
        let _ = self
            .storage
            .record_failed_attempt(failed_attempts, locked_until)
            .await;

        if lockout > 0 {
            VaultError::LockedOut { seconds: lockout }
        } else {
            VaultError::IncorrectMasterPassword
        }
    }

    /// Derives the KEK and unwraps the vault master key.
    /// `Ok(None)` means "this password is wrong"; `Err` means the header itself
    /// is unusable.
    fn unwrap_with(
        &self,
        meta: &VaultMeta,
        password: &MasterPassword,
    ) -> VaultResult<Option<VaultKey>> {
        let kek = meta.kdf.derive(password)?;
        match key::unwrap_key(&kek, &meta.wrapped_key) {
            Ok(key) => Ok(Some(key)),
            Err(VaultError::Tampered) => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Locks the vault, wiping the in-memory vault master key.
    pub async fn lock(&self) -> bool {
        self.session.lock().await
    }

    /// Extends the idle deadline. Called on user activity in the vault UI.
    pub fn touch(&self) {
        self.session.touch();
    }

    pub fn set_auto_lock_seconds(&self, seconds: u64) {
        self.session.set_auto_lock_seconds(seconds);
    }

    /// Locks the vault if the idle timeout has passed. Returns whether it locked.
    pub async fn sweep_auto_lock(&self) -> bool {
        self.session.sweep().await
    }

    // ------------------------------------------------------------------ items

    /// Lists item summaries. Requires an unlocked vault.
    pub async fn list_items(&self) -> VaultResult<Vec<VaultItemSummary>> {
        let key = self.require_key().await?;
        let records = self.storage.list_records().await?;
        let mut summaries = Vec::with_capacity(records.len());
        for record in records {
            // Decrypting to build the list is unavoidable without a plaintext
            // index, and a plaintext index is exactly what must not exist. Each
            // decrypted item is dropped, and wiped, at the end of the iteration.
            let item = decrypt_record(&key, &record)?;
            summaries.push(item.summary());
        }
        Ok(summaries)
    }

    /// Reads one item.
    ///
    /// The returned item carries **no secret values**: the password and any
    /// sensitive custom field are stripped here, before serialisation. Everything
    /// the detail pane displays is present; anything masked is not. A secret is
    /// obtained one field at a time through
    /// [`reveal_field`](Self::reveal_field), only when the user asks for it.
    pub async fn get_item(&self, id: &str) -> VaultResult<Option<VaultItem>> {
        let key = self.require_key().await?;
        let Some(record) = self.storage.get_record(id).await? else {
            return Ok(None);
        };
        Ok(Some(decrypt_record(&key, &record)?.without_secrets()))
    }

    /// Adds a new item. The record id is generated here, never supplied by the
    /// caller, so it cannot be chosen to collide with an existing row.
    pub async fn create_item(&self, payload: VaultItemPayload) -> VaultResult<VaultItem> {
        let key = self.require_key().await?;
        let payload = payload.normalize()?;

        let id = uuid::Uuid::new_v4().to_string();
        let now = crate::now_millis();
        let record = encrypt_payload(&key, &id, &payload, now, now)?;
        self.storage.put_record(&record).await?;
        // The caller already holds what it just submitted, so echoing the secrets
        // back would only widen their exposure for no benefit.
        Ok(VaultItem::from_parts(id, now, now, payload).without_secrets())
    }

    /// Replaces an existing item's contents.
    pub async fn update_item(&self, id: &str, payload: VaultItemPayload) -> VaultResult<VaultItem> {
        let key = self.require_key().await?;
        let payload = payload.normalize()?;

        let existing = self
            .storage
            .get_record(id)
            .await?
            .ok_or_else(|| VaultError::Validation("vault item no longer exists".into()))?;
        let now = crate::now_millis();
        let record = encrypt_payload(&key, id, &payload, existing.created_at, now)?;
        self.storage.put_record(&record).await?;
        Ok(
            VaultItem::from_parts(id.to_owned(), existing.created_at, now, payload)
                .without_secrets(),
        )
    }

    /// Toggles an item's favourite flag without moving any secret across the
    /// boundary.
    ///
    /// The flag lives inside the encrypted payload, so this has to decrypt and
    /// re-encrypt the record. Doing it natively means the UI never has to hold —
    /// or re-submit — the password just to mark an item as a favourite.
    pub async fn set_favorite(&self, id: &str, favorite: bool) -> VaultResult<VaultItem> {
        let key = self.require_key().await?;
        let existing = self
            .storage
            .get_record(id)
            .await?
            .ok_or_else(|| VaultError::Validation("vault item no longer exists".into()))?;

        let mut payload = decrypt_payload(&key, &existing)?;
        payload.favorite = favorite;
        let now = crate::now_millis();
        let record = encrypt_payload(&key, id, &payload, existing.created_at, now)?;
        self.storage.put_record(&record).await?;
        Ok(
            VaultItem::from_parts(id.to_owned(), existing.created_at, now, payload)
                .without_secrets(),
        )
    }

    /// Reads exactly one secret field, on explicit request.
    ///
    /// `field_index` of `None` selects the item's password; `Some(index)` selects
    /// a sensitive custom field. The value is returned in locked memory and is
    /// never bundled with the rest of the item, so a caller that renders a list or
    /// a detail pane cannot receive a secret by accident.
    pub async fn reveal_field(
        &self,
        id: &str,
        field_index: Option<usize>,
    ) -> VaultResult<Option<LockedBuffer>> {
        let key = self.require_key().await?;
        let Some(record) = self.storage.get_record(id).await? else {
            return Ok(None);
        };
        let payload = decrypt_payload(&key, &record)?;

        let value = match field_index {
            None => payload.password.as_deref(),
            Some(index) => payload
                .custom_fields
                .get(index)
                .filter(|field| field.sensitive)
                .map(|field| field.value.as_str()),
        };
        let Some(value) = value.filter(|value| !value.is_empty()) else {
            return Ok(None);
        };
        Ok(Some(LockedBuffer::from_text(value)))
    }

    /// Deletes an item. The UI is responsible for confirming first; the vault
    /// deliberately does not know how to undo this.
    pub async fn delete_item(&self, id: &str) -> VaultResult<bool> {
        let _key = self.require_key().await?;
        self.storage.delete_record(id).await
    }

    /// Removes the vault from this machine entirely: every record, the wrapped
    /// vault master key, and the KDF parameters.
    ///
    /// **This deliberately does not require the master password.** Deleting a
    /// local file never did — anyone with access to the machine can remove
    /// `vault.db` directly — so a password prompt here would be security theatre
    /// while also defeating the one case that needs this operation: a vault that
    /// cannot be unlocked, for instance a backup restored with a password its
    /// owner no longer has. The guard against an accidental wipe is therefore a
    /// deliberate typed confirmation in the UI, and it is described as such
    /// rather than presented as a security control.
    ///
    /// Requires an existing vault, and locks the session first so no key survives
    /// the operation.
    pub async fn destroy(&self) -> VaultResult<()> {
        if self.storage.load_meta().await?.is_none() {
            return Err(VaultError::NotInitialized);
        }
        self.session.lock().await;
        self.storage.erase_all().await
    }

    // --------------------------------------------------------- master password

    /// Re-wraps the vault master key under a new master password.
    ///
    /// The vault master key is *not* regenerated, so no record is re-encrypted:
    /// only the small wrapper around the key changes. A fresh salt is generated
    /// and the calibrated cost settings are preserved.
    pub async fn change_master_password(
        &self,
        current: &MasterPassword,
        new: &MasterPassword,
    ) -> VaultResult<()> {
        new.validate()?;
        // Require an unlocked session as defence in depth: a locked vault cannot
        // have its password changed at all.
        let _key = self.require_key().await?;
        // Changing the password verifies the current one, so it is another place
        // a master password can be guessed at. It shares the throttle.
        self.refuse_while_locked_out().await?;

        let meta = self
            .storage
            .load_meta()
            .await?
            .ok_or(VaultError::NotInitialized)?;
        let Some(vault_master_key) = self.unwrap_with(&meta, current)? else {
            return Err(self.register_failure().await);
        };

        // Changing the master password also repairs a cost that predates the
        // current floor. This is the one moment a user is guaranteed to be
        // re-deriving anyway, so raising the cost here is free, and it gives a
        // remedy for a vault created by an earlier build.
        let new_params = meta.kdf.upgraded()?;
        let new_kek = new_params.derive(new)?;
        let wrapped_key = key::wrap_key(&new_kek, &vault_master_key)?;

        let updated = VaultMeta {
            format_version: meta.format_version,
            kdf: new_params,
            wrapped_key,
            created_at: meta.created_at,
            updated_at: crate::now_millis(),
        };
        self.storage.update_meta(&updated).await
    }

    /// Re-benchmarks the key-derivation cost against this machine and re-wraps the
    /// vault master key under the result.
    ///
    /// The remedy for a vault whose parameters are weaker than the hardware can
    /// afford — most commonly one created by an earlier build, or on a machine
    /// that has since been replaced. A master-password change only lifts a vault to
    /// the floor; this lifts it to whatever the machine actually measures.
    ///
    /// **No record is touched.** The vault master key is *not* regenerated, so only
    /// the small wrapper around it is rewritten — the same property that makes a
    /// master-password change cheap, and the reason this is safe on a vault of any
    /// size. The cost is never lowered: see [`KdfParams::recalibrated`].
    ///
    /// Requires the master password, because re-wrapping needs a KEK and only the
    /// password can produce one. It shares the unlock throttle for the same reason:
    /// it verifies a password, so it is another place one could be guessed at.
    pub async fn recalibrate(&self, password: &MasterPassword) -> VaultResult<VaultStatus> {
        let _key = self.require_key().await?;
        self.refuse_while_locked_out().await?;

        let meta = self
            .storage
            .load_meta()
            .await?
            .ok_or(VaultError::NotInitialized)?;
        let Some(vault_master_key) = self.unwrap_with(&meta, password)? else {
            return Err(self.register_failure().await);
        };

        let new_params = meta.kdf.recalibrated()?;
        let new_kek = new_params.derive(password)?;
        let wrapped_key = key::wrap_key(&new_kek, &vault_master_key)?;

        let updated = VaultMeta {
            format_version: meta.format_version,
            kdf: new_params,
            wrapped_key,
            created_at: meta.created_at,
            updated_at: crate::now_millis(),
        };
        self.storage.update_meta(&updated).await?;
        self.status().await
    }

    /// Rotates the vault master key, re-encrypting every record.
    ///
    /// Reserved by the format for a future release and reachable from tests, but
    /// deliberately not exposed in the UI yet. The whole rotation — new key, new
    /// wrapped key, every record rewritten — commits in a single transaction, so
    /// a crash cannot leave the vault half under the old key and half under the
    /// new one.
    ///
    /// Requires the master password because the new vault master key has to be
    /// wrapped under the KEK, and only the password can produce it.
    pub async fn rotate_vault_key(&self, password: &MasterPassword) -> VaultResult<()> {
        let _key = self.require_key().await?;
        self.refuse_while_locked_out().await?;

        let meta = self
            .storage
            .load_meta()
            .await?
            .ok_or(VaultError::NotInitialized)?;
        let Some(old_key) = self.unwrap_with(&meta, password)? else {
            return Err(self.register_failure().await);
        };

        let new_key = VaultKey::generate();
        let records = self.storage.list_records().await?;
        let mut reencrypted = Vec::with_capacity(records.len());
        for record in &records {
            let payload = decrypt_payload(&old_key, record)?;
            reencrypted.push(encrypt_payload(
                &new_key,
                &record.id,
                &payload,
                record.created_at,
                record.updated_at,
            )?);
        }

        let kek = meta.kdf.derive(password)?;
        let updated = VaultMeta {
            format_version: meta.format_version,
            kdf: meta.kdf.clone(),
            wrapped_key: key::wrap_key(&kek, &new_key)?,
            created_at: meta.created_at,
            updated_at: crate::now_millis(),
        };
        self.storage.replace_all(&updated, &reencrypted).await?;

        self.session.unlock(new_key).await;
        Ok(())
    }

    // ----------------------------------------------------------------- backup

    /// Produces an encrypted backup document.
    ///
    /// Everything the vault needs to be restored is included, and nothing else:
    /// the salt and KDF parameters, the wrapped vault master key, and every item
    /// exactly as it is stored on disk — still encrypted. There is no plaintext
    /// export path anywhere in this module, by design.
    ///
    /// Unlocking is not required: a backup is a copy of ciphertext.
    pub async fn export_backup(&self) -> VaultResult<String> {
        let meta = self
            .storage
            .load_meta()
            .await?
            .ok_or(VaultError::NotInitialized)?;
        let records = self.storage.list_records().await?;

        let document = BackupDocument {
            magic: BACKUP_MAGIC.to_owned(),
            format_version: BACKUP_FORMAT_VERSION,
            exported_at: crate::now_millis(),
            vault_format_version: meta.format_version,
            vault_created_at: meta.created_at,
            kdf: BackupKdf {
                algorithm: meta.kdf.algorithm.clone(),
                version: meta.kdf.version,
                salt: BASE64.encode(&meta.kdf.salt),
                memory_kib: meta.kdf.memory_kib,
                time_cost: meta.kdf.time_cost,
                parallelism: meta.kdf.parallelism,
                output_len: meta.kdf.output_len,
            },
            wrapped_key_nonce: BASE64.encode(&meta.wrapped_key.nonce),
            wrapped_key_blob: BASE64.encode(&meta.wrapped_key.ciphertext),
            items: records.iter().map(BackupItem::from_record).collect(),
        };
        serde_json::to_string_pretty(&document)
            .map_err(|_| VaultError::Backup("backup could not be serialized"))
    }

    /// Restores a vault from an encrypted backup.
    ///
    /// `replace_existing` decides what happens when a vault is already present.
    /// When it is `false` the import is refused, so a caller cannot overwrite live
    /// credentials by accident. When it is `true` the existing vault is replaced —
    /// which is the only way to recover from a vault that cannot be unlocked, or
    /// to roll back to an older backup.
    ///
    /// Replacing is deliberately explicit, and the caller is expected to have
    /// preserved the current vault first: this method cannot do that itself
    /// without knowing where the user keeps backups. See the `vault_import_backup`
    /// command, which writes a safety copy before asking for a replace.
    ///
    /// Either way, **everything is validated before a single byte is written**, so
    /// a corrupt or crafted backup leaves the current vault untouched.
    pub async fn import_backup(
        &self,
        document: &str,
        replace_existing: bool,
    ) -> VaultResult<VaultBackupSummary> {
        if self.storage.is_initialized().await? && !replace_existing {
            return Err(VaultError::AlreadyInitialized);
        }
        let document: BackupDocument = serde_json::from_str(document)
            .map_err(|_| VaultError::Backup("backup file is not readable"))?;
        if document.magic != BACKUP_MAGIC || document.format_version != BACKUP_FORMAT_VERSION {
            return Err(VaultError::Backup(
                "backup file is not a Dev Workbench vault",
            ));
        }

        let meta = VaultMeta {
            format_version: document.vault_format_version,
            kdf: KdfParams {
                algorithm: document.kdf.algorithm,
                version: document.kdf.version,
                salt: decode(&document.kdf.salt)?,
                memory_kib: document.kdf.memory_kib,
                time_cost: document.kdf.time_cost,
                parallelism: document.kdf.parallelism,
                output_len: document.kdf.output_len,
            },
            wrapped_key: key::WrappedKey {
                nonce: decode(&document.wrapped_key_nonce)?,
                ciphertext: decode(&document.wrapped_key_blob)?,
            },
            created_at: document.vault_created_at,
            updated_at: crate::now_millis(),
        };
        let records = document
            .items
            .iter()
            .map(BackupItem::to_record)
            .collect::<VaultResult<Vec<_>>>()?;

        // Validate everything before writing a single byte. A backup carrying an
        // unsupported KDF, a short salt, or a structurally impossible record would
        // otherwise be written successfully and produce a vault that can never be
        // opened — a self-inflicted lockout from a restore, which is the worst
        // possible time to discover a problem.
        meta.kdf.validate()?;
        validate_wrapped_key(&meta.wrapped_key)?;
        for record in &records {
            validate_record(record)?;
        }

        // A fresh file: `replace_all` writes the header and every record in one
        // transaction, so a failed import leaves no half-restored vault.
        self.storage.replace_all(&meta, &records).await?;
        Ok(VaultBackupSummary {
            item_count: records.len(),
            format_version: meta.format_version,
        })
    }

    // -------------------------------------------------------------- generator

    /// Generates a password from the OS CSPRNG. Nothing is persisted here; the
    /// caller decides whether to apply the result to an item.
    pub fn generate_password(&self, options: &GeneratorOptions) -> VaultResult<SecretString> {
        crate::generator::generate(options)
    }

    // ---------------------------------------------------------------- private

    async fn require_key(&self) -> VaultResult<VaultKey> {
        self.session.key().await.ok_or(VaultError::Locked)
    }
}

// ------------------------------------------------------------------ helpers

/// Associated data for a record. Binds the ciphertext to this exact row and to
/// this vault format version.
fn item_aad(id: &str) -> Vec<u8> {
    let mut aad = Vec::with_capacity(ITEM_AAD_PREFIX.len() + id.len());
    aad.extend_from_slice(ITEM_AAD_PREFIX);
    aad.extend_from_slice(id.as_bytes());
    aad
}

/// Serializes and encrypts one item payload. The intermediate JSON buffer is
/// wiped once the ciphertext exists.
fn encrypt_payload(
    key: &VaultKey,
    id: &str,
    payload: &VaultItemPayload,
    created_at: i64,
    updated_at: i64,
) -> VaultResult<StoredRecord> {
    let plaintext = Zeroizing::new(
        serde_json::to_vec(payload)
            .map_err(|_| VaultError::Crypto("vault item could not be serialized"))?,
    );
    let (nonce, ciphertext) = crypto::seal(key, &item_aad(id), &plaintext)?;
    Ok(StoredRecord {
        id: id.to_owned(),
        nonce,
        ciphertext,
        record_version: ITEM_RECORD_VERSION,
        created_at,
        updated_at,
    })
}

fn decrypt_payload(key: &VaultKey, record: &StoredRecord) -> VaultResult<VaultItemPayload> {
    if record.record_version != ITEM_RECORD_VERSION {
        return Err(VaultError::Corrupt("unsupported record version"));
    }
    let plaintext = crypto::open(
        key,
        &item_aad(&record.id),
        &record.nonce,
        &record.ciphertext,
    )?;
    serde_json::from_slice(&plaintext)
        .map_err(|_| VaultError::Corrupt("vault item could not be decoded"))
}

fn decrypt_record(key: &VaultKey, record: &StoredRecord) -> VaultResult<VaultItem> {
    let payload = decrypt_payload(key, record)?;
    Ok(VaultItem::from_parts(
        record.id.clone(),
        record.created_at,
        record.updated_at,
        payload,
    ))
}

fn decode(value: &str) -> VaultResult<Vec<u8>> {
    BASE64
        .decode(value)
        .map_err(|_| VaultError::Backup("backup file contains unreadable data"))
}

/// Rejects a wrapped key that could never have been produced by this vault.
fn validate_wrapped_key(wrapped: &key::WrappedKey) -> VaultResult<()> {
    if wrapped.nonce.len() != crypto::NONCE_LEN {
        return Err(VaultError::Backup("backup has an unusable wrapped key"));
    }
    // A sealed 32-byte key is exactly a key plus an authentication tag.
    if wrapped.ciphertext.len() != crate::key::KEY_LEN + crypto::TAG_LEN {
        return Err(VaultError::Backup("backup has an unusable wrapped key"));
    }
    Ok(())
}

/// Rejects a record that could not be decrypted even with the right key.
///
/// This is a structural check only: whether the ciphertext is *genuine* is decided
/// by the AEAD tag at read time, and cannot be decided here without the key.
fn validate_record(record: &StoredRecord) -> VaultResult<()> {
    if record.record_version != ITEM_RECORD_VERSION {
        return Err(VaultError::Backup("backup contains an unsupported record"));
    }
    if record.nonce.len() != crypto::NONCE_LEN || record.ciphertext.len() < crypto::TAG_LEN {
        return Err(VaultError::Backup("backup contains an unusable record"));
    }
    if record.id.is_empty() {
        return Err(VaultError::Backup("backup contains a record without an id"));
    }
    Ok(())
}

/// On-disk shape of an exported backup. Every field is either public
/// (parameters, timestamps) or ciphertext.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackupDocument {
    magic: String,
    format_version: u32,
    exported_at: i64,
    vault_format_version: u32,
    vault_created_at: i64,
    kdf: BackupKdf,
    wrapped_key_nonce: String,
    wrapped_key_blob: String,
    items: Vec<BackupItem>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackupKdf {
    algorithm: String,
    version: u32,
    salt: String,
    memory_kib: u32,
    time_cost: u32,
    parallelism: u32,
    output_len: usize,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackupItem {
    id: String,
    nonce: String,
    ciphertext: String,
    record_version: u32,
    created_at: i64,
    updated_at: i64,
}

impl BackupItem {
    fn from_record(record: &StoredRecord) -> Self {
        Self {
            id: record.id.clone(),
            nonce: BASE64.encode(&record.nonce),
            ciphertext: BASE64.encode(&record.ciphertext),
            record_version: record.record_version,
            created_at: record.created_at,
            updated_at: record.updated_at,
        }
    }

    fn to_record(&self) -> VaultResult<StoredRecord> {
        Ok(StoredRecord {
            id: self.id.clone(),
            nonce: decode(&self.nonce)?,
            ciphertext: decode(&self.ciphertext)?,
            record_version: self.record_version,
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}

/// Parses a generator request that arrived as untyped JSON, so the Tauri layer
/// can accept partial options without duplicating the defaults.
pub fn generator_options_from_value(value: Value) -> VaultResult<GeneratorOptions> {
    if value.is_null() {
        return Ok(GeneratorOptions::default());
    }
    serde_json::from_value(value)
        .map_err(|_| VaultError::Validation("invalid password generator options".into()))
}
