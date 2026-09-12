//! Security test-suite for the Password Vault.
//!
//! These tests are written against the *threat model*, not against the
//! implementation. Each one names the attacker capability it denies, so that a
//! future refactor which quietly weakens a guarantee fails a test whose intent
//! is obvious from its name.
//!
//! Requirements 15 and 16 of the module specification — that plugins and AI
//! cannot reach vault data — are boundary properties of the TypeScript packages
//! and live in `packages/core/src/index.test.ts` and
//! `apps/desktop/src/stores/vault.test.ts`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use secrecy::ExposeSecret;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{Row, SqlitePool};
use workbench_vault::kdf::KdfParams;
use workbench_vault::model::VaultItemField;
use workbench_vault::{
    GeneratorOptions, MasterPassword, VaultError, VaultItemPayload, VaultService,
};

/// Distinguishable secrets, so a byte scan cannot match them by accident and a
/// failure names the field that leaked.
const MASTER_PASSWORD: &str = "correct-horse-battery-staple-42";
const NEW_MASTER_PASSWORD: &str = "a-completely-different-passphrase-7";
const TITLE: &str = "GITHUB-PLATFORM-TITLE-MARKER";
const USERNAME: &str = "jensen@example.com-USERNAME-MARKER";
const PASSWORD: &str = "S3cr3t-PASSWORD-MARKER-do-not-log";
const NOTES: &str = "NOTES-MARKER-recovery-codes-live-elsewhere";
const URL: &str = "https://github.example.com-URL-MARKER";
const TAG: &str = "TAG-MARKER-work";
const FIELD_VALUE: &str = "CUSTOM-FIELD-MARKER-acme-org";
const SECOND_PASSWORD: &str = "SECOND-ITEM-PASSWORD-MARKER";
const THIRD_PASSWORD: &str = "THIRD-ITEM-PASSWORD-MARKER";

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A vault in a private temporary directory, removed on drop.
struct TempVault {
    dir: PathBuf,
}

impl TempVault {
    fn new(label: &str) -> Self {
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "dev-workbench-vault-test-{label}-{}-{unique}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        Self { dir }
    }

    fn db_path(&self) -> PathBuf {
        self.dir.join("vault.db")
    }

    /// Every byte SQLite may have written, including the write-ahead log and the
    /// shared-memory file. Scanning only `vault.db` would miss the WAL, which is
    /// exactly where a careless design leaks plaintext.
    fn all_bytes(&self) -> Vec<u8> {
        let mut collected = Vec::new();
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return collected;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file()
                && let Ok(mut bytes) = std::fs::read(&path)
            {
                collected.append(&mut bytes);
            }
        }
        collected
    }

    /// Fails when `needle` appears anywhere in the vault's on-disk bytes.
    fn assert_absent(&self, needle: &str, what: &str) {
        assert!(
            !contains(&self.all_bytes(), needle),
            "{what} was found in plaintext on disk; the vault must only persist ciphertext"
        );
    }

    fn file_names(&self) -> Vec<String> {
        std::fs::read_dir(&self.dir)
            .map(|entries| {
                entries
                    .flatten()
                    .map(|entry| entry.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default()
    }
}

impl Drop for TempVault {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn contains(haystack: &[u8], needle: &str) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle.as_bytes())
}

fn master(value: &str) -> MasterPassword {
    MasterPassword::new(value.to_owned())
}

fn payload(title: &str, password: &str) -> VaultItemPayload {
    VaultItemPayload {
        title: title.to_owned(),
        username: Some(USERNAME.to_owned()),
        password: Some(password.to_owned()),
        url: Some(URL.to_owned()),
        notes: Some(NOTES.to_owned()),
        tags: vec![TAG.to_owned()],
        custom_fields: vec![VaultItemField {
            name: "organisation".to_owned(),
            value: FIELD_VALUE.to_owned(),
            sensitive: true,
        }],
        favorite: true,
    }
}

/// Creates an initialised, unlocked vault holding one marker item.
async fn seeded_vault(label: &str) -> (TempVault, VaultService) {
    let temp = TempVault::new(label);
    let service = VaultService::open(&temp.db_path()).await.expect("open vault");
    service
        .create_with_params(master(MASTER_PASSWORD), KdfParams::insecure_for_tests())
        .await
        .expect("create vault");
    service
        .create_item(payload(TITLE, PASSWORD))
        .await
        .expect("create item");
    (temp, service)
}

/// Reads an item's password the only way the service now allows: one explicit
/// request for one field, returned in locked memory.
async fn password_of(service: &VaultService, id: &str) -> Option<String> {
    service
        .reveal_field(id, None)
        .await
        .expect("reveal")
        .map(|value| value.as_str().to_owned())
}

// ---------------------------------------------------------------------------
// Direct database access, used to simulate an attacker who can edit the file.
// ---------------------------------------------------------------------------

struct StoredRow {
    id: String,
    nonce: Vec<u8>,
    ciphertext: Vec<u8>,
    updated_at: i64,
}

async fn raw_pool(path: &Path) -> SqlitePool {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(false);
    SqlitePool::connect_with(options)
        .await
        .expect("open raw sqlite")
}

async fn stored_rows(path: &Path) -> Vec<StoredRow> {
    let pool = raw_pool(path).await;
    let rows = sqlx::query("SELECT id, nonce, ciphertext, updated_at FROM vault_items ORDER BY id")
        .fetch_all(&pool)
        .await
        .expect("read records");
    pool.close().await;
    rows.iter()
        .map(|row| StoredRow {
            id: row.get("id"),
            nonce: row.get("nonce"),
            ciphertext: row.get("ciphertext"),
            updated_at: row.get("updated_at"),
        })
        .collect()
}

/// Applies `mutate` to the first stored record and writes it back, preserving
/// lengths so the SQLite page layout stays valid.
async fn tamper_first_record(path: &Path, mutate: impl Fn(&mut Vec<u8>, &mut Vec<u8>)) {
    let pool = raw_pool(path).await;
    let row: (String, Vec<u8>, Vec<u8>) =
        sqlx::query_as("SELECT id, nonce, ciphertext FROM vault_items ORDER BY id LIMIT 1")
            .fetch_one(&pool)
            .await
            .expect("fetch record");
    let (id, mut nonce, mut ciphertext) = row;
    mutate(&mut nonce, &mut ciphertext);
    sqlx::query("UPDATE vault_items SET nonce=?, ciphertext=? WHERE id=?")
        .bind(&nonce)
        .bind(&ciphertext)
        .bind(&id)
        .execute(&pool)
        .await
        .expect("write tampered record");
    pool.close().await;
}

// =====================================================================
// Requirements 1-4: the vault file contains no plaintext secret fields
// =====================================================================

#[tokio::test]
async fn vault_file_never_contains_the_master_password() {
    let (temp, service) = seeded_vault("master-password").await;
    let _ = service;
    temp.assert_absent(MASTER_PASSWORD, "the master password");
}

#[tokio::test]
async fn vault_file_never_contains_the_username() {
    let (temp, service) = seeded_vault("username").await;
    let _ = service;
    temp.assert_absent(USERNAME, "the username");
}

#[tokio::test]
async fn vault_file_never_contains_the_password() {
    let (temp, service) = seeded_vault("password").await;
    let _ = service;
    temp.assert_absent(PASSWORD, "a stored password");
}

#[tokio::test]
async fn vault_file_never_contains_the_notes() {
    let (temp, service) = seeded_vault("notes").await;
    let _ = service;
    temp.assert_absent(NOTES, "the notes");
}

#[tokio::test]
async fn vault_file_never_contains_even_the_title_url_tag_or_custom_field() {
    // Titles, URLs, tags and custom fields are sensitive too: a plaintext index
    // of them would be exactly the metadata leak this module must avoid.
    let (temp, service) = seeded_vault("metadata").await;
    let _ = service;
    temp.assert_absent(TITLE, "the item title");
    temp.assert_absent(URL, "the item URL");
    temp.assert_absent(TAG, "a tag");
    temp.assert_absent(FIELD_VALUE, "a custom field value");
}

#[tokio::test]
async fn the_vault_never_writes_plaintext_to_a_sidecar_file() {
    let (temp, service) = seeded_vault("sidecar").await;
    // Exercise a write path that touches the write-ahead log, then scan every
    // file in the directory rather than only the main database.
    service
        .create_item(payload("Third", THIRD_PASSWORD))
        .await
        .expect("create");
    service.lock().await;

    let names = temp.file_names();
    assert!(
        names.iter().any(|name| name == "vault.db"),
        "the vault database should exist, found {names:?}"
    );
    let bytes = temp.all_bytes();
    for needle in [PASSWORD, THIRD_PASSWORD, NOTES, USERNAME, MASTER_PASSWORD] {
        assert!(
            !contains(&bytes, needle),
            "plaintext leaked into one of {names:?}"
        );
    }
}

// =====================================================================
// Requirements 5-7: unlock, refusal, and tamper detection
// =====================================================================

#[tokio::test]
async fn the_correct_master_password_decrypts() {
    let (temp, service) = seeded_vault("correct-password").await;
    service.lock().await;

    let status = service
        .unlock(master(MASTER_PASSWORD))
        .await
        .expect("unlock with the right password");
    assert!(status.unlocked);
    assert_eq!(status.item_count, 1);

    let items = service.list_items().await.expect("list");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].title, TITLE);

    let item = service
        .get_item(&items[0].id)
        .await
        .expect("get")
        .expect("present");
    assert!(
        item.password.is_none(),
        "a fetched item must never carry a password"
    );
    assert!(item.has_password);
    assert_eq!(
        password_of(&service, &items[0].id).await.as_deref(),
        Some(PASSWORD)
    );
    drop(temp);
}

#[tokio::test]
async fn a_fetched_item_never_carries_a_secret_value() {
    // The least-privilege invariant: rendering a detail pane must not require the
    // password or any sensitive custom field to enter the UI process. Only an
    // explicit per-field request may produce one.
    let (temp, service) = seeded_vault("item-redaction").await;
    let items = service.list_items().await.expect("list");
    let item = service
        .get_item(&items[0].id)
        .await
        .expect("get")
        .expect("present");

    // Everything the detail pane displays is present.
    assert_eq!(item.title, TITLE);
    assert_eq!(item.username.as_deref(), Some(USERNAME));
    assert_eq!(item.url.as_deref(), Some(URL));
    assert_eq!(item.notes.as_deref(), Some(NOTES));
    assert_eq!(item.tags, vec![TAG]);
    assert_eq!(item.custom_fields[0].name, "organisation");

    // Nothing masked is.
    assert!(item.password.is_none());
    assert!(item.has_password, "presence is still reported");
    assert!(
        item.custom_fields[0].sensitive,
        "the sensitive flag survives so the UI can mask it"
    );
    assert_eq!(item.custom_fields[0].value, "");

    // The serialized form cannot leak either.
    let encoded = serde_json::to_string(&item).expect("serialize");
    assert!(!encoded.contains(PASSWORD));
    assert!(!encoded.contains(FIELD_VALUE));

    // Reading a value back requires asking for that field specifically.
    assert_eq!(
        password_of(&service, &items[0].id).await.as_deref(),
        Some(PASSWORD)
    );
    let field = service
        .reveal_field(&items[0].id, Some(0))
        .await
        .expect("reveal field")
        .expect("present");
    assert_eq!(field.as_str(), FIELD_VALUE);
    drop(temp);
}

#[tokio::test]
async fn reveal_only_yields_sensitive_fields_and_only_for_the_named_item() {
    let (temp, service) = seeded_vault("reveal-scope").await;
    let items = service.list_items().await.expect("list");
    let id = items[0].id.clone();

    // An out-of-range index, or another item, yields nothing rather than data.
    assert!(
        service
            .reveal_field(&id, Some(99))
            .await
            .expect("reveal")
            .is_none()
    );
    assert!(
        service
            .reveal_field("no-such-item", None)
            .await
            .expect("reveal")
            .is_none()
    );

    // A non-sensitive field is deliberately not revealable: it is already part of
    // the item, and exposing it here would create a second path to the same data.
    let mut public = payload("Public", "");
    public.custom_fields[0].sensitive = false;
    let created = service.create_item(public).await.expect("create");
    assert!(
        service
            .reveal_field(&created.id, Some(0))
            .await
            .expect("reveal")
            .is_none()
    );

    // An item with no password reveals nothing, and reports no password.
    assert!(!created.has_password);
    assert!(
        service
            .reveal_field(&created.id, None)
            .await
            .expect("reveal")
            .is_none()
    );
    drop(temp);
}

#[tokio::test]
async fn favouriting_an_item_does_not_require_its_password() {
    // Before this was a native operation the UI had to fetch the whole item and
    // send it back to flip one boolean — which would now wipe the password.
    let (temp, service) = seeded_vault("favorite").await;
    let items = service.list_items().await.expect("list");
    let id = items[0].id.clone();

    let updated = service.set_favorite(&id, false).await.expect("favorite off");
    assert!(!updated.favorite);
    assert!(updated.password.is_none(), "no secret comes back either");

    // The credential itself must be untouched by the round trip.
    assert_eq!(password_of(&service, &id).await.as_deref(), Some(PASSWORD));
    let item = service.get_item(&id).await.expect("get").expect("present");
    assert_eq!(item.title, TITLE);
    assert_eq!(item.notes.as_deref(), Some(NOTES));

    let restored = service.set_favorite(&id, true).await.expect("favorite on");
    assert!(restored.favorite);
    assert_eq!(password_of(&service, &id).await.as_deref(), Some(PASSWORD));
    drop(temp);
}

#[tokio::test]
async fn updating_an_item_without_a_password_clears_it_deliberately() {
    // Documents the one place a caller *must* supply the secret: an edit replaces
    // the record wholesale. A fetched item has been redacted, so it cannot be
    // round-tripped as an update by itself.
    let (temp, service) = seeded_vault("edit-clears").await;
    let items = service.list_items().await.expect("list");
    let id = items[0].id.clone();

    let fetched = service.get_item(&id).await.expect("get").expect("present");
    assert!(fetched.password.is_none());

    let mut edited = payload("Renamed", "");
    edited.username = fetched.username.clone();
    service.update_item(&id, edited).await.expect("update");

    let after = service.get_item(&id).await.expect("get").expect("present");
    assert_eq!(after.title, "Renamed");
    assert!(!after.has_password, "an omitted password clears the field");
    assert!(password_of(&service, &id).await.is_none());
    drop(temp);
}

#[tokio::test]
async fn a_wrong_master_password_cannot_decrypt() {
    let (temp, service) = seeded_vault("wrong-password").await;
    service.lock().await;

    let error = service
        .unlock(master("this-is-not-the-right-password"))
        .await
        .expect_err("a wrong password must fail");
    assert!(
        matches!(error, VaultError::IncorrectMasterPassword),
        "must be the single uniform error, got {error:?}"
    );
    assert_eq!(error.to_string(), "Incorrect master password.");
    assert!(!service.status().await.expect("status").unlocked);
    drop(temp);
}

#[tokio::test]
async fn the_wrong_password_error_leaks_no_cryptographic_detail() {
    let (temp, service) = seeded_vault("error-shape").await;
    service.lock().await;

    let error = service
        .unlock(master("another-wrong-password-entirely"))
        .await
        .expect_err("must fail");
    let rendered = format!("{error} {error:?}").to_ascii_lowercase();
    for leak in ["argon", "tag", "nonce", "salt", "kek", "poly1305", "vmk"] {
        assert!(
            !rendered.contains(leak),
            "the error surfaced an implementation detail ({leak}): {rendered}"
        );
    }
    drop(temp);
}

#[tokio::test]
async fn a_modified_ciphertext_cannot_be_decrypted() {
    let (temp, service) = seeded_vault("tamper-ciphertext").await;
    service.lock().await;
    let _ = service;

    tamper_first_record(&temp.db_path(), |_nonce, ciphertext| {
        ciphertext[0] ^= 0x01;
    })
    .await;

    let service = VaultService::open(&temp.db_path()).await.expect("reopen");
    service
        .unlock(master(MASTER_PASSWORD))
        .await
        .expect("the header is intact, so unlocking still works");
    let error = service
        .list_items()
        .await
        .expect_err("a tampered record must not decrypt");
    assert!(
        matches!(error, VaultError::Tampered | VaultError::Corrupt(_)),
        "tampering must be detected, got {error:?}"
    );
    drop(temp);
}

#[tokio::test]
async fn a_modified_nonce_cannot_be_decrypted() {
    let (temp, service) = seeded_vault("tamper-nonce").await;
    service.lock().await;
    let _ = service;

    tamper_first_record(&temp.db_path(), |nonce, _ciphertext| {
        nonce[0] ^= 0x80;
    })
    .await;

    let service = VaultService::open(&temp.db_path()).await.expect("reopen");
    service
        .unlock(master(MASTER_PASSWORD))
        .await
        .expect("the header is intact");
    let error = service
        .list_items()
        .await
        .expect_err("a modified nonce must not decrypt");
    assert!(matches!(error, VaultError::Tampered | VaultError::Corrupt(_)));
    drop(temp);
}

#[tokio::test]
async fn a_modified_authentication_tag_cannot_be_decrypted() {
    let (temp, service) = seeded_vault("tamper-tag").await;
    service.lock().await;
    let _ = service;

    tamper_first_record(&temp.db_path(), |_nonce, ciphertext| {
        let last = ciphertext.len() - 1;
        ciphertext[last] ^= 0xff;
    })
    .await;

    let service = VaultService::open(&temp.db_path()).await.expect("reopen");
    service.unlock(master(MASTER_PASSWORD)).await.expect("unlock");
    assert!(service.list_items().await.is_err());
    drop(temp);
}

#[tokio::test]
async fn a_record_cannot_be_relocated_to_a_different_item_id() {
    // Associated data binds a ciphertext to its row id, so copying ciphertext
    // between rows fails authentication instead of silently decrypting.
    let (temp, service) = seeded_vault("aad-binding").await;
    let second = service
        .create_item(payload("Second", SECOND_PASSWORD))
        .await
        .expect("create");
    service.lock().await;
    let _ = service;

    let source = stored_rows(&temp.db_path()).await;
    let donor = source
        .iter()
        .find(|row| row.id == second.id)
        .expect("second record");
    let pool = raw_pool(&temp.db_path()).await;
    sqlx::query("UPDATE vault_items SET nonce=?, ciphertext=? WHERE id<>?")
        .bind(&donor.nonce)
        .bind(&donor.ciphertext)
        .bind(&second.id)
        .execute(&pool)
        .await
        .expect("relocate ciphertext");
    pool.close().await;

    let service = VaultService::open(&temp.db_path()).await.expect("reopen");
    service.unlock(master(MASTER_PASSWORD)).await.expect("unlock");
    let error = service
        .list_items()
        .await
        .expect_err("a relocated record must not authenticate");
    assert!(matches!(error, VaultError::Tampered | VaultError::Corrupt(_)));
    drop(temp);
}

// =====================================================================
// Requirement 8: nonces are never reused
// =====================================================================

#[tokio::test]
async fn identical_plaintext_never_produces_a_repeated_nonce_or_ciphertext() {
    let temp = TempVault::new("nonce-uniqueness");
    let service = VaultService::open(&temp.db_path()).await.expect("open");
    service
        .create_with_params(master(MASTER_PASSWORD), KdfParams::insecure_for_tests())
        .await
        .expect("create");

    // The very same secret, many times over.
    for _ in 0..48 {
        service
            .create_item(payload(TITLE, PASSWORD))
            .await
            .expect("create item");
    }
    let _ = service;

    let rows = stored_rows(&temp.db_path()).await;
    assert_eq!(rows.len(), 48);

    let mut nonces: Vec<&Vec<u8>> = rows.iter().map(|row| &row.nonce).collect();
    nonces.sort();
    let before = nonces.len();
    nonces.dedup();
    assert_eq!(before, nonces.len(), "a nonce was reused under one key");

    let mut ciphertexts: Vec<&Vec<u8>> = rows.iter().map(|row| &row.ciphertext).collect();
    ciphertexts.sort();
    let before = ciphertexts.len();
    ciphertexts.dedup();
    assert_eq!(
        before,
        ciphertexts.len(),
        "identical plaintext must still produce distinct ciphertext"
    );

    temp.assert_absent(PASSWORD, "the shared plaintext");
    drop(temp);
}

// =====================================================================
// Requirement 9: changing the master password
// =====================================================================

#[tokio::test]
async fn changing_the_master_password_invalidates_the_old_one() {
    let (temp, service) = seeded_vault("change-password").await;

    service
        .change_master_password(&master(MASTER_PASSWORD), &master(NEW_MASTER_PASSWORD))
        .await
        .expect("change password");
    service.lock().await;

    let old = service
        .unlock(master(MASTER_PASSWORD))
        .await
        .expect_err("the old password must stop working");
    assert!(matches!(old, VaultError::IncorrectMasterPassword));

    service
        .unlock(master(NEW_MASTER_PASSWORD))
        .await
        .expect("the new password must work");
    let items = service.list_items().await.expect("list");
    assert_eq!(items.len(), 1);
    let item = service
        .get_item(&items[0].id)
        .await
        .expect("get")
        .expect("present");
    assert!(item.password.is_none());
    assert_eq!(
        password_of(&service, &items[0].id).await.as_deref(),
        Some(PASSWORD),
        "records must survive a password change untouched"
    );
    drop(temp);
}

#[tokio::test]
async fn changing_the_master_password_does_not_re_encrypt_records() {
    // The vault master key is unchanged, so no record ciphertext should move.
    let (temp, service) = seeded_vault("change-password-cheap").await;
    let before = stored_rows(&temp.db_path()).await;

    service
        .change_master_password(&master(MASTER_PASSWORD), &master(NEW_MASTER_PASSWORD))
        .await
        .expect("change password");

    let after = stored_rows(&temp.db_path()).await;
    assert_eq!(before.len(), after.len());
    for (before, after) in before.iter().zip(after.iter()) {
        assert_eq!(before.id, after.id);
        assert_eq!(before.nonce, after.nonce);
        assert_eq!(before.ciphertext, after.ciphertext);
    }
    drop(temp);
}

#[tokio::test]
async fn changing_the_master_password_requires_the_current_one() {
    let (temp, service) = seeded_vault("change-password-guard").await;

    let error = service
        .change_master_password(
            &master("not-the-current-password"),
            &master(NEW_MASTER_PASSWORD),
        )
        .await
        .expect_err("must reject a wrong current password");
    assert!(matches!(error, VaultError::IncorrectMasterPassword));

    service.lock().await;
    service
        .unlock(master(MASTER_PASSWORD))
        .await
        .expect("the original password still works");
    drop(temp);
}

#[tokio::test]
async fn changing_the_master_password_stores_neither_password() {
    let (temp, service) = seeded_vault("change-password-leak").await;
    service
        .change_master_password(&master(MASTER_PASSWORD), &master(NEW_MASTER_PASSWORD))
        .await
        .expect("change password");
    let _ = service;
    temp.assert_absent(NEW_MASTER_PASSWORD, "the new master password");
    temp.assert_absent(MASTER_PASSWORD, "the old master password");
}

#[tokio::test]
async fn a_weak_new_master_password_is_rejected() {
    let (temp, service) = seeded_vault("change-password-weak").await;
    let error = service
        .change_master_password(&master(MASTER_PASSWORD), &master("short"))
        .await
        .expect_err("must reject a weak new password");
    assert!(matches!(error, VaultError::Validation(_)));
    drop(temp);
}

#[tokio::test]
async fn changing_the_master_password_requires_an_unlocked_vault() {
    let (temp, service) = seeded_vault("change-password-locked").await;
    service.lock().await;
    let error = service
        .change_master_password(&master(MASTER_PASSWORD), &master(NEW_MASTER_PASSWORD))
        .await
        .expect_err("must refuse while locked");
    assert!(matches!(error, VaultError::Locked));
    drop(temp);
}

// =====================================================================
// Requirement 10: locking clears in-memory state
// =====================================================================

#[tokio::test]
async fn locking_clears_the_in_memory_key_and_refuses_reads() {
    let (temp, service) = seeded_vault("lock-clears").await;
    assert!(service.status().await.expect("status").unlocked);
    assert!(service.lock().await, "the vault was unlocked");

    let status = service.status().await.expect("status");
    assert!(!status.unlocked);
    assert!(matches!(service.list_items().await, Err(VaultError::Locked)));
    assert!(matches!(
        service.get_item("anything").await,
        Err(VaultError::Locked)
    ));
    assert!(matches!(
        service.create_item(payload(TITLE, PASSWORD)).await,
        Err(VaultError::Locked)
    ));
    assert!(matches!(
        service.delete_item("anything").await,
        Err(VaultError::Locked)
    ));
    assert!(!service.lock().await, "locking twice is a no-op");
    drop(temp);
}

#[tokio::test]
async fn the_idle_timeout_locks_the_vault_and_drops_its_key() {
    let (temp, service) = seeded_vault("auto-lock").await;
    service.set_auto_lock_seconds(1);
    assert!(!service.sweep_auto_lock().await, "not idle yet");

    std::thread::sleep(std::time::Duration::from_millis(1_100));

    assert!(service.sweep_auto_lock().await, "the timeout must lock");
    assert!(!service.status().await.expect("status").unlocked);
    assert!(matches!(service.list_items().await, Err(VaultError::Locked)));
    drop(temp);
}

// =====================================================================
// Key rotation (architecture reserved for a future release)
// =====================================================================

#[tokio::test]
async fn rotating_the_vault_key_rewrites_records_and_keeps_the_password() {
    let (temp, service) = seeded_vault("rotate").await;
    let before = stored_rows(&temp.db_path()).await;

    service
        .rotate_vault_key(&master(MASTER_PASSWORD))
        .await
        .expect("rotate");

    let items = service.list_items().await.expect("list");
    let item = service
        .get_item(&items[0].id)
        .await
        .expect("get")
        .expect("present");
    assert!(item.password.is_none());
    assert_eq!(
        password_of(&service, &items[0].id).await.as_deref(),
        Some(PASSWORD),
        "rotation must not lose data"
    );

    let after = stored_rows(&temp.db_path()).await;
    assert_eq!(before.len(), after.len());
    assert_ne!(
        before[0].ciphertext, after[0].ciphertext,
        "rotation must produce new ciphertext"
    );

    // The master password is unchanged by a key rotation.
    service.lock().await;
    service
        .unlock(master(MASTER_PASSWORD))
        .await
        .expect("the same master password still unlocks");
    drop(temp);
}

#[tokio::test]
async fn rotating_the_vault_key_requires_the_master_password() {
    let (temp, service) = seeded_vault("rotate-guard").await;
    let error = service
        .rotate_vault_key(&master("definitely-not-the-password"))
        .await
        .expect_err("must reject a wrong password");
    assert!(matches!(error, VaultError::IncorrectMasterPassword));
    drop(temp);
}

// =====================================================================
// Encrypted backup: permitted, but never plaintext
// =====================================================================

#[tokio::test]
async fn a_backup_is_encrypted_and_restores_the_vault() {
    let (temp, service) = seeded_vault("backup").await;
    let backup = service.export_backup().await.expect("export");

    for (needle, what) in [
        (PASSWORD, "a password"),
        (USERNAME, "a username"),
        (NOTES, "notes"),
        (TITLE, "a title"),
        (MASTER_PASSWORD, "the master password"),
    ] {
        assert!(
            !backup.contains(needle),
            "the backup exported {what} in plaintext"
        );
    }
    assert!(backup.contains("dev-workbench.vault.backup"));

    let restored_dir = TempVault::new("backup-restore");
    let restored = VaultService::open(&restored_dir.db_path())
        .await
        .expect("open restored");
    let summary = restored.import_backup(&backup, false).await.expect("import");
    assert_eq!(summary.item_count, 1);

    restored
        .unlock(master(MASTER_PASSWORD))
        .await
        .expect("the original master password unlocks the restore");
    let items = restored.list_items().await.expect("list");
    let item = restored
        .get_item(&items[0].id)
        .await
        .expect("get")
        .expect("present");
    assert!(item.password.is_none());
    assert_eq!(
        password_of(&restored, &items[0].id).await.as_deref(),
        Some(PASSWORD)
    );
    assert_eq!(item.notes.as_deref(), Some(NOTES));
    restored.lock().await;

    restored_dir.assert_absent(PASSWORD, "a restored password");
    restored_dir.assert_absent(MASTER_PASSWORD, "the master password of a restore");
    drop(temp);
}

#[tokio::test]
async fn importing_refuses_to_overwrite_an_existing_vault() {
    let (temp, service) = seeded_vault("import-guard").await;
    let backup = service.export_backup().await.expect("export");
    let error = service
        .import_backup(&backup, false)
        .await
        .expect_err("must refuse to clobber a live vault");
    assert!(matches!(error, VaultError::AlreadyInitialized));
    drop(temp);
}

#[tokio::test]
async fn a_backup_of_a_different_product_is_rejected() {
    let temp = TempVault::new("backup-reject");
    let service = VaultService::open(&temp.db_path()).await.expect("open");
    let error = service
        .import_backup("{\"magic\":\"not-ours\",\"formatVersion\":1}", false)
        .await
        .expect_err("must reject a foreign file");
    assert!(matches!(error, VaultError::Backup(_)));
    drop(temp);
}

#[tokio::test]
async fn a_vault_cannot_be_created_twice() {
    let (temp, service) = seeded_vault("double-create").await;
    let error = service
        .create_with_params(
            master("a-brand-new-master-password"),
            KdfParams::insecure_for_tests(),
        )
        .await
        .expect_err("must not overwrite key material");
    assert!(matches!(error, VaultError::AlreadyInitialized));
    drop(temp);
}

#[tokio::test]
async fn operating_on_a_missing_vault_is_reported_clearly() {
    let temp = TempVault::new("missing");
    let service = VaultService::open(&temp.db_path()).await.expect("open");
    let status = service.status().await.expect("status");
    assert!(!status.exists);
    assert!(!status.unlocked);
    assert!(matches!(
        service.unlock(master(MASTER_PASSWORD)).await,
        Err(VaultError::NotInitialized)
    ));
    assert!(matches!(
        service.export_backup().await,
        Err(VaultError::NotInitialized)
    ));
    drop(temp);
}

// =====================================================================
// Item lifecycle and least privilege
// =====================================================================

#[tokio::test]
async fn the_list_projection_never_carries_a_password_or_note() {
    let (temp, service) = seeded_vault("list-projection").await;
    let items = service.list_items().await.expect("list");
    let encoded = serde_json::to_string(&items).expect("serialize");

    assert!(!encoded.contains(PASSWORD), "the list must not carry passwords");
    assert!(!encoded.contains(NOTES), "the list must not carry notes");
    assert!(
        !encoded.contains(FIELD_VALUE),
        "the list must not carry custom field values"
    );
    assert!(encoded.contains("\"hasPassword\":true"));
    drop(temp);
}

#[tokio::test]
async fn items_can_be_created_updated_and_deleted() {
    let (temp, service) = seeded_vault("crud").await;
    let created = service
        .create_item(payload("Second", SECOND_PASSWORD))
        .await
        .expect("create");
    assert_eq!(service.list_items().await.expect("list").len(), 2);

    let mut updated_payload = payload("Second renamed", SECOND_PASSWORD);
    updated_payload.favorite = false;
    let updated = service
        .update_item(&created.id, updated_payload)
        .await
        .expect("update");
    assert_eq!(updated.title, "Second renamed");
    assert!(!updated.favorite);
    assert_eq!(updated.created_at, created.created_at);

    assert!(service.delete_item(&created.id).await.expect("delete"));
    assert!(service.get_item(&created.id).await.expect("get").is_none());
    assert!(!service.delete_item(&created.id).await.expect("re-delete"));
    drop(temp);
}

#[tokio::test]
async fn an_item_payload_is_validated_before_it_is_encrypted() {
    let (temp, service) = seeded_vault("validation").await;
    let mut invalid = payload("   ", PASSWORD);
    invalid.tags.clear();
    let error = service
        .create_item(invalid)
        .await
        .expect_err("an empty title must be rejected");
    assert!(matches!(error, VaultError::Validation(_)));
    assert_eq!(
        service.list_items().await.expect("list").len(),
        1,
        "the invalid item must not have been stored"
    );
    drop(temp);
}

#[tokio::test]
async fn a_vault_reopens_with_its_items_intact() {
    // Simulates restarting the application: a fresh service over the same file
    // must start locked and then see the same data.
    let (temp, service) = seeded_vault("reopen").await;
    drop(service);

    let reopened = VaultService::open(&temp.db_path()).await.expect("reopen");
    let status = reopened.status().await.expect("status");
    assert!(status.exists);
    assert!(!status.unlocked, "a restart must always start locked");
    assert_eq!(status.item_count, 1);

    reopened
        .unlock(master(MASTER_PASSWORD))
        .await
        .expect("unlock");
    let items = reopened.list_items().await.expect("list");
    assert_eq!(items[0].title, TITLE);
    drop(temp);
}

// =====================================================================
// Password generator
// =====================================================================

#[tokio::test]
async fn the_generator_stores_nothing_and_is_not_persisted() {
    let (temp, service) = seeded_vault("generator").await;
    let generated = service
        .generate_password(&GeneratorOptions {
            length: 32,
            ..Default::default()
        })
        .expect("generate");
    let value = generated.expose_secret().to_owned();
    assert_eq!(value.chars().count(), 32);

    // A generated password is not persisted until the user applies it.
    let _ = service;
    temp.assert_absent(&value, "a generated password");
}

// =====================================================================
// Destroying the vault: the escape hatch for an unopenable vault
// =====================================================================

#[tokio::test]
async fn destroying_the_vault_removes_every_credential_and_the_key() {
    let (temp, service) = seeded_vault("destroy").await;
    let id = service.list_items().await.expect("list")[0].id.clone();
    assert_eq!(password_of(&service, &id).await.as_deref(), Some(PASSWORD));

    service.destroy().await.expect("destroy");

    let status = service.status().await.expect("status");
    assert!(!status.exists, "the vault must no longer exist");
    assert!(!status.unlocked);
    assert_eq!(status.item_count, 0);
    assert!(status.kdf.is_none(), "the KDF parameters go with it");

    // Nothing is readable, and the old master password no longer opens anything.
    assert!(matches!(service.list_items().await, Err(VaultError::Locked)));
    assert!(matches!(
        service.unlock(master(MASTER_PASSWORD)).await,
        Err(VaultError::NotInitialized)
    ));
    temp.assert_absent(PASSWORD, "a destroyed password");
    temp.assert_absent(MASTER_PASSWORD, "the master password of a destroyed vault");
    drop(temp);
}

#[tokio::test]
async fn a_destroyed_vault_can_be_replaced_by_a_different_one() {
    // The point of the escape hatch: after removing an unopenable vault, the user
    // can create a new one or restore a backup in the same location.
    let (temp, service) = seeded_vault("destroy-then-create").await;
    service.destroy().await.expect("destroy");

    service
        .create_with_params(
            master(NEW_MASTER_PASSWORD),
            KdfParams::insecure_for_tests(),
        )
        .await
        .expect("create a replacement vault");

    // The replacement is genuinely new: the old password does not open it, and
    // none of the old credentials are present.
    service.lock().await;
    assert!(matches!(
        service.unlock(master(MASTER_PASSWORD)).await,
        Err(VaultError::IncorrectMasterPassword)
    ));
    service
        .unlock(master(NEW_MASTER_PASSWORD))
        .await
        .expect("the new password opens the new vault");
    assert!(
        service.list_items().await.expect("list").is_empty(),
        "the previous vault's items must not survive"
    );
    drop(temp);
}

#[tokio::test]
async fn a_destroyed_vault_can_be_replaced_by_a_restored_backup() {
    // The second dead end this closes: a backup restored with a password its owner
    // no longer has can be removed and a different backup restored in its place.
    let (temp, service) = seeded_vault("destroy-then-restore").await;
    let backup = service.export_backup().await.expect("export");
    service.destroy().await.expect("destroy");

    assert!(
        !service.status().await.expect("status").exists,
        "import must be permitted again after a destroy"
    );
    let summary = service.import_backup(&backup, false).await.expect("import");
    assert_eq!(summary.item_count, 1);

    service
        .unlock(master(MASTER_PASSWORD))
        .await
        .expect("the restored vault opens");
    let items = service.list_items().await.expect("list");
    assert_eq!(items[0].title, TITLE);
    drop(temp);
}

#[tokio::test]
async fn destroying_without_a_vault_is_reported_rather_than_silently_accepted() {
    let temp = TempVault::new("destroy-missing");
    let service = VaultService::open(&temp.db_path()).await.expect("open");
    assert!(matches!(
        service.destroy().await,
        Err(VaultError::NotInitialized)
    ));
    drop(temp);
}

#[tokio::test]
async fn destroying_leaves_the_write_ahead_log_free_of_ciphertext() {
    // Deleting rows is not enough on its own: in WAL mode the removed content
    // would still sit in the -wal sidecar. This asserts the checkpoint and
    // rewrite actually clear it.
    let (temp, service) = seeded_vault("destroy-wal").await;
    let marker = {
        // Record the exact ciphertext of a record before the destroy.
        let rows = stored_rows(&temp.db_path()).await;
        rows[0].ciphertext.clone()
    };
    service.destroy().await.expect("destroy");
    let _ = service;

    let bytes = temp.all_bytes();
    assert!(
        !bytes.windows(marker.len()).any(|window| window == marker),
        "the removed record's ciphertext survived the destroy"
    );
    assert!(!contains(&bytes, PASSWORD));
    drop(temp);
}

// =====================================================================
// Replacing an existing vault with a backup
// =====================================================================

#[tokio::test]
async fn importing_over_an_existing_vault_requires_the_flag() {
    // The default must stay safe: a stray import cannot destroy live credentials.
    let (temp, service) = seeded_vault("replace-guard").await;
    let backup = service.export_backup().await.expect("export");

    let error = service
        .import_backup(&backup, false)
        .await
        .expect_err("an unflagged import must be refused");
    assert!(matches!(error, VaultError::AlreadyInitialized));

    // And the existing vault is untouched.
    assert_eq!(password_of(&service, &service.list_items().await.expect("list")[0].id).await.as_deref(), Some(PASSWORD));
    drop(temp);
}

#[tokio::test]
async fn a_flagged_import_replaces_the_existing_vault() {
    // The path that makes restoring reachable at all: an existing vault, which is
    // the only situation a user with a vault is ever in.
    let (temp, service) = seeded_vault("replace-existing").await;
    let id = service.list_items().await.expect("list")[0].id.clone();
    assert_eq!(password_of(&service, &id).await.as_deref(), Some(PASSWORD));

    // Build a second, independent vault and take its backup.
    let donor_temp = TempVault::new("replace-donor");
    let donor = VaultService::open(&donor_temp.db_path()).await.expect("open");
    donor
        .create_with_params(master(NEW_MASTER_PASSWORD), KdfParams::insecure_for_tests())
        .await
        .expect("create donor");
    donor
        .create_item(payload("Donor", SECOND_PASSWORD))
        .await
        .expect("create donor item");
    let donor_backup = donor.export_backup().await.expect("export donor");

    let summary = service
        .import_backup(&donor_backup, true)
        .await
        .expect("replace");
    assert_eq!(summary.item_count, 1);

    // The replacement is complete: the old password no longer opens it, the new
    // one does, and the old credentials are gone.
    service.lock().await;
    assert!(matches!(
        service.unlock(master(MASTER_PASSWORD)).await,
        Err(VaultError::IncorrectMasterPassword)
    ));
    service
        .unlock(master(NEW_MASTER_PASSWORD))
        .await
        .expect("the imported vault opens with its own password");
    let items = service.list_items().await.expect("list");
    assert_eq!(items[0].title, "Donor");
    assert_eq!(password_of(&service, &items[0].id).await.as_deref(), Some(SECOND_PASSWORD));

    // The replaced vault is gone from disk, not merely shadowed.
    temp.assert_absent(PASSWORD, "a replaced password");
    drop(donor_temp);
    drop(temp);
}

#[tokio::test]
async fn a_refused_replace_leaves_the_existing_vault_intact() {
    // Validation still runs before anything is written, which matters far more
    // once replacing is permitted: a bad file must not cost the user their vault.
    let (temp, service) = seeded_vault("replace-refused").await;
    let id = service.list_items().await.expect("list")[0].id.clone();

    let good = service.export_backup().await.expect("export");
    let tampered = mutate_backup(&good, |document| {
        document["items"][0]["nonce"] = serde_json::Value::String("AAAA".into());
    });
    let error = service
        .import_backup(&tampered, true)
        .await
        .expect_err("a damaged backup must not replace anything");
    assert!(matches!(error, VaultError::Backup(_)));

    // The original vault is still there, still openable, still holding its item.
    let items = service.list_items().await.expect("list");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, id);
    assert_eq!(password_of(&service, &id).await.as_deref(), Some(PASSWORD));
    drop(temp);
}

// =====================================================================
// Unlock throttling
// =====================================================================

/// Moves an active lockout into the past, so escalation can be tested without
/// waiting it out.
async fn expire_lockout(path: &Path) {
    let pool = raw_pool(path).await;
    sqlx::query("UPDATE vault_meta SET locked_until=0 WHERE id=1")
        .execute(&pool)
        .await
        .expect("expire lockout");
    pool.close().await;
}

/// Fails an unlock `count` times, returning the last error.
async fn fail_unlock(service: &VaultService, count: u32) -> VaultError {
    let mut last = VaultError::IncorrectMasterPassword;
    for _ in 0..count {
        last = service
            .unlock(master("not-the-master-password"))
            .await
            .expect_err("a wrong password must fail");
    }
    last
}

#[tokio::test]
async fn early_failures_are_answered_with_the_uniform_password_error() {
    // Below the threshold the answer must stay indistinguishable from any other
    // wrong password — no hint that a lockout is approaching.
    let (temp, service) = seeded_vault("throttle-early").await;
    service.lock().await;

    for attempt in 1..workbench_vault::lockout::LOCKOUT_THRESHOLD {
        let error = fail_unlock(&service, 1).await;
        assert!(
            matches!(error, VaultError::IncorrectMasterPassword),
            "attempt {attempt} should not reveal a lockout"
        );
        let status = service.status().await.expect("status");
        assert_eq!(status.failed_attempts, attempt);
        assert!(status.locked_until.is_none());
    }
    drop(temp);
}

#[tokio::test]
async fn the_threshold_failure_locks_the_vault_for_five_minutes() {
    let (temp, service) = seeded_vault("throttle-threshold").await;
    service.lock().await;

    let error = fail_unlock(&service, workbench_vault::lockout::LOCKOUT_THRESHOLD).await;
    match error {
        VaultError::LockedOut { seconds } => assert_eq!(seconds, 300),
        other => panic!("expected a lockout, got {other:?}"),
    }

    let status = service.status().await.expect("status");
    assert!(status.locked_until.is_some());
    assert_eq!(status.failed_attempts, workbench_vault::lockout::LOCKOUT_THRESHOLD);
    drop(temp);
}

#[tokio::test]
async fn a_locked_out_vault_refuses_even_the_correct_password() {
    // The lockout has to be unconditional: letting the right password through
    // would make it a hint that the password was right.
    let (temp, service) = seeded_vault("throttle-correct").await;
    service.lock().await;
    fail_unlock(&service, workbench_vault::lockout::LOCKOUT_THRESHOLD).await;

    let error = service
        .unlock(master(MASTER_PASSWORD))
        .await
        .expect_err("a lockout must refuse everything");
    assert!(matches!(error, VaultError::LockedOut { .. }));
    assert!(!service.status().await.expect("status").unlocked);
    drop(temp);
}

#[tokio::test]
async fn the_lockout_survives_restarting_the_application() {
    // Throttling that a restart clears would not be throttling. This is why the
    // counter lives in the vault header rather than in memory.
    let (temp, service) = seeded_vault("throttle-restart").await;
    service.lock().await;
    fail_unlock(&service, workbench_vault::lockout::LOCKOUT_THRESHOLD).await;
    drop(service);

    let reopened = VaultService::open(&temp.db_path()).await.expect("reopen");
    let status = reopened.status().await.expect("status");
    assert_eq!(status.failed_attempts, workbench_vault::lockout::LOCKOUT_THRESHOLD);
    assert!(status.locked_until.is_some(), "the lockout must persist");
    assert!(
        matches!(
            reopened.unlock(master(MASTER_PASSWORD)).await,
            Err(VaultError::LockedOut { .. })
        ),
        "reopening must not hand out a fresh set of attempts"
    );
    drop(temp);
}

#[tokio::test]
async fn a_successful_unlock_clears_the_throttle() {
    let (temp, service) = seeded_vault("throttle-clear").await;
    service.lock().await;
    fail_unlock(&service, workbench_vault::lockout::LOCKOUT_THRESHOLD - 1).await;
    expire_lockout(&temp.db_path()).await;

    service
        .unlock(master(MASTER_PASSWORD))
        .await
        .expect("the correct password still works below the threshold");

    let status = service.status().await.expect("status");
    assert_eq!(status.failed_attempts, 0, "a success must reset the counter");
    assert!(status.locked_until.is_none());
    drop(temp);
}

#[tokio::test]
async fn a_failure_after_an_expired_lockout_escalates() {
    // The core of "后面再错就叠加锁定时间": the counter is not reset by the
    // lockout merely expiring.
    let (temp, service) = seeded_vault("throttle-escalate").await;
    service.lock().await;
    fail_unlock(&service, workbench_vault::lockout::LOCKOUT_THRESHOLD).await;
    expire_lockout(&temp.db_path()).await;

    let error = fail_unlock(&service, 1).await;
    match error {
        VaultError::LockedOut { seconds } => {
            assert_eq!(seconds, 600, "the second lockout must be twice the first")
        }
        other => panic!("expected an escalated lockout, got {other:?}"),
    }

    expire_lockout(&temp.db_path()).await;
    let error = fail_unlock(&service, 1).await;
    match error {
        VaultError::LockedOut { seconds } => assert_eq!(seconds, 1_200),
        other => panic!("expected a further escalated lockout, got {other:?}"),
    }
    drop(temp);
}

#[tokio::test]
async fn the_lockout_never_exceeds_its_ceiling() {
    // With no master-password recovery, an uncapped policy could make the vault
    // permanently unreachable to its owner.
    let (temp, service) = seeded_vault("throttle-cap").await;
    let id = service.list_items().await.expect("list")[0].id.clone();
    assert_eq!(password_of(&service, &id).await.as_deref(), Some(PASSWORD));
    service.lock().await;

    let mut last = 0;
    for _ in 0..12 {
        fail_unlock(&service, workbench_vault::lockout::LOCKOUT_THRESHOLD).await;
        expire_lockout(&temp.db_path()).await;
        if let Err(VaultError::LockedOut { seconds }) =
            service.unlock(master("still-wrong")).await
        {
            last = seconds;
        }
        expire_lockout(&temp.db_path()).await;
    }
    assert_eq!(last, workbench_vault::lockout::MAX_LOCKOUT_SECONDS);

    // And the rightful owner can still get back in once the lockout expires.
    service
        .unlock(master(MASTER_PASSWORD))
        .await
        .expect("the owner must never be permanently locked out");
    assert_eq!(service.status().await.expect("status").failed_attempts, 0);
    drop(temp);
}

#[tokio::test]
async fn changing_the_master_password_shares_the_throttle() {
    // Otherwise the lockout would be trivially side-stepped by guessing through
    // the change-password form instead of the unlock form.
    let (temp, service) = seeded_vault("throttle-change").await;
    fail_unlock(&service, workbench_vault::lockout::LOCKOUT_THRESHOLD - 1).await;

    let error = service
        .change_master_password(&master("wrong-current-password"), &master(NEW_MASTER_PASSWORD))
        .await
        .expect_err("a wrong current password must fail");
    assert!(
        matches!(error, VaultError::LockedOut { .. }),
        "the change-password path must count towards the same throttle, got {error:?}"
    );
    drop(temp);
}

#[tokio::test]
async fn a_locked_out_vault_refuses_password_guessing_through_every_path() {
    let (temp, service) = seeded_vault("throttle-paths").await;
    service.lock().await;
    fail_unlock(&service, workbench_vault::lockout::LOCKOUT_THRESHOLD).await;

    assert!(matches!(
        service
            .change_master_password(&master(MASTER_PASSWORD), &master(NEW_MASTER_PASSWORD))
            .await,
        Err(VaultError::LockedOut { .. }) | Err(VaultError::Locked)
    ));
    assert!(matches!(
        service.rotate_vault_key(&master(MASTER_PASSWORD)).await,
        Err(VaultError::LockedOut { .. }) | Err(VaultError::Locked)
    ));
    drop(temp);
}

#[tokio::test]
async fn the_escape_hatch_still_works_while_locked_out() {
    // The lockout must never remove the way out, or a vault that cannot be
    // unlocked would be both unreachable and impossible to remove.
    let (temp, service) = seeded_vault("throttle-escape").await;
    service.lock().await;
    fail_unlock(&service, workbench_vault::lockout::LOCKOUT_THRESHOLD).await;

    service
        .destroy()
        .await
        .expect("removing the vault must not be blocked by a lockout");
    assert!(!service.status().await.expect("status").exists);
    drop(temp);
}

// =====================================================================
// Re-calibrating an existing vault in place
// =====================================================================

#[tokio::test]
async fn recalibrating_strengthens_the_parameters_without_touching_a_record() {
    // The remedy for a vault created below the floor: reach what the machine can
    // afford, keeping every credential exactly as it is.
    let (temp, service) = seeded_vault("recalibrate").await;
    let before_rows = stored_rows(&temp.db_path()).await;
    let before_kdf = service.status().await.expect("status").kdf.expect("kdf");
    assert!(!before_kdf.meets_current_floor, "the fixture starts below the floor");

    let after = service.recalibrate(&master(MASTER_PASSWORD)).await.expect("recalibrate");
    let after_kdf = after.kdf.expect("kdf");

    assert!(after_kdf.meets_current_floor);
    assert!(
        after_kdf.memory_kib >= before_kdf.memory_kib,
        "recalibration must never weaken the vault"
    );

    // The decisive property: no record was re-encrypted. Same ids, same nonces,
    // same ciphertext, same timestamps.
    let after_rows = stored_rows(&temp.db_path()).await;
    assert_eq!(before_rows.len(), after_rows.len());
    for (before, after) in before_rows.iter().zip(after_rows.iter()) {
        assert_eq!(before.id, after.id);
        assert_eq!(before.nonce, after.nonce, "a record was re-encrypted");
        assert_eq!(before.ciphertext, after.ciphertext, "a record was rewritten");
        assert_eq!(before.updated_at, after.updated_at);
    }
    drop(temp);
}

#[tokio::test]
async fn a_recalibrated_vault_still_opens_with_the_same_password_and_data() {
    let (temp, service) = seeded_vault("recalibrate-unlock").await;
    let id = service.list_items().await.expect("list")[0].id.clone();

    service.recalibrate(&master(MASTER_PASSWORD)).await.expect("recalibrate");

    service.lock().await;
    service
        .unlock(master(MASTER_PASSWORD))
        .await
        .expect("the same password must still open the vault");
    let items = service.list_items().await.expect("list");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, id);
    assert_eq!(
        password_of(&service, &id).await.as_deref(),
        Some(PASSWORD),
        "the credential must survive with the same vault key"
    );
    drop(temp);
}

#[tokio::test]
async fn recalibrating_requires_the_master_password() {
    let (temp, service) = seeded_vault("recalibrate-guard").await;
    let error = service
        .recalibrate(&master("not-the-master-password"))
        .await
        .expect_err("a wrong password must be refused");
    assert!(matches!(error, VaultError::IncorrectMasterPassword));
    // And nothing was written: the original password still works.
    service.lock().await;
    service.unlock(master(MASTER_PASSWORD)).await.expect("unlock");
    drop(temp);
}

#[tokio::test]
async fn recalibrating_requires_an_unlocked_vault() {
    let (temp, service) = seeded_vault("recalibrate-locked").await;
    service.lock().await;
    let error = service
        .recalibrate(&master(MASTER_PASSWORD))
        .await
        .expect_err("must refuse while locked");
    assert!(matches!(error, VaultError::Locked));
    drop(temp);
}

#[tokio::test]
async fn recalibrating_shares_the_unlock_throttle() {
    // It verifies a password, so it must be another path that cannot be used to
    // guess one.
    let (temp, service) = seeded_vault("recalibrate-throttle").await;
    fail_unlock(&service, workbench_vault::lockout::LOCKOUT_THRESHOLD).await;

    // `unlock` locked it out; recalibration must refuse for the same reason.
    service
        .recalibrate(&master(MASTER_PASSWORD))
        .await
        .expect_err("a locked-out vault must not recalibrate");
    drop(temp);
}

// =====================================================================
// Test-only parameter guard
// =====================================================================

#[tokio::test]
async fn the_production_entry_point_never_picks_the_test_parameters() {
    let temp = TempVault::new("calibration-guard");
    let service = VaultService::open(&temp.db_path()).await.expect("open");
    service
        .create(master(MASTER_PASSWORD))
        .await
        .expect("create with calibrated parameters");

    let kdf = service.status().await.expect("status").kdf.expect("kdf");
    let weak = KdfParams::insecure_for_tests();
    assert!(
        kdf.memory_kib > weak.memory_kib || kdf.time_cost > weak.time_cost,
        "production defaults must be materially stronger than the test parameters"
    );
    assert_eq!(kdf.algorithm, "argon2id");
    assert!(
        kdf.meets_current_floor,
        "a vault created through the production entry point must meet the security floor"
    );
    temp.assert_absent(MASTER_PASSWORD, "the master password");
    drop(temp);
}

// =====================================================================
// Security floor, and the remedy for vaults created below it
// =====================================================================

#[tokio::test]
async fn a_vault_below_the_floor_is_reported_but_still_opens() {
    // Backwards compatibility: a vault created by an earlier build stored weaker
    // parameters. It must keep working, and the status must say so plainly rather
    // than silently pretending the vault is as strong as a new one.
    let (temp, service) = seeded_vault("below-floor").await;

    let kdf = service.status().await.expect("status").kdf.expect("kdf");
    assert!(
        !kdf.meets_current_floor,
        "the test vault is deliberately built below the floor"
    );

    service.lock().await;
    service
        .unlock(master(MASTER_PASSWORD))
        .await
        .expect("a below-floor vault must still unlock");
    assert_eq!(service.list_items().await.expect("list").len(), 1);
    drop(temp);
}

#[tokio::test]
async fn changing_the_master_password_repairs_a_below_floor_vault() {
    // The remedy: changing the master password re-derives anyway, so it also
    // raises a weak cost to the current floor. Without this, a vault created
    // below the floor would stay weak forever.
    let (temp, service) = seeded_vault("floor-upgrade").await;
    assert!(
        !service
            .status()
            .await
            .expect("status")
            .kdf
            .expect("kdf")
            .meets_current_floor
    );

    service
        .change_master_password(&master(MASTER_PASSWORD), &master(NEW_MASTER_PASSWORD))
        .await
        .expect("change password");

    let kdf = service.status().await.expect("status").kdf.expect("kdf");
    assert!(
        kdf.meets_current_floor,
        "changing the master password must raise the cost to the floor"
    );

    // The stronger parameters must be usable: the vault still opens with the new
    // password, still refuses the old one, and still holds its data.
    service.lock().await;
    assert!(matches!(
        service.unlock(master(MASTER_PASSWORD)).await,
        Err(VaultError::IncorrectMasterPassword)
    ));
    service
        .unlock(master(NEW_MASTER_PASSWORD))
        .await
        .expect("the new password unlocks the upgraded vault");
    let items = service.list_items().await.expect("list");
    let item = service
        .get_item(&items[0].id)
        .await
        .expect("get")
        .expect("present");
    assert!(item.password.is_none());
    assert_eq!(
        password_of(&service, &items[0].id).await.as_deref(),
        Some(PASSWORD)
    );
    drop(temp);
}

#[tokio::test]
async fn the_vault_master_key_is_held_in_locked_memory() {
    // Wiping a key on drop only helps once it is gone; while it is alive it must
    // not be pageable to disk. This asserts the lock actually took effect rather
    // than trusting that the call succeeded.
    let (temp, service) = seeded_vault("locked-memory").await;

    let key = service
        .session()
        .key()
        .await
        .expect("the vault is unlocked, so a key is available");
    if cfg!(any(windows, unix)) {
        assert!(
            key.is_memory_locked(),
            "the vault master key must be pinned out of the pagefile"
        );
    }

    // The same must hold for a key derived and unwrapped fresh from the password.
    service.lock().await;
    service
        .unlock(master(MASTER_PASSWORD))
        .await
        .expect("unlock");
    let key = service.session().key().await.expect("key");
    if cfg!(any(windows, unix)) {
        assert!(key.is_memory_locked());
    }
    drop(temp);
}

// =====================================================================
// A restore must never leave an unopenable vault behind
// =====================================================================

/// Rewrites one field of an exported backup document, so a malformed file can be
/// fed back in without hand-writing the whole envelope.
fn mutate_backup(backup: &str, mutate: impl Fn(&mut serde_json::Value)) -> String {
    let mut document: serde_json::Value =
        serde_json::from_str(backup).expect("backup is valid json");
    mutate(&mut document);
    serde_json::to_string(&document).expect("re-serialize")
}

#[tokio::test]
async fn importing_a_backup_with_an_unsupported_kdf_is_refused() {
    // A crafted or corrupted backup naming a KDF this build cannot derive with
    // must be refused *before* anything is written. Accepting it would create a
    // vault that can never be opened, which is the worst moment to find out.
    let (temp, service) = seeded_vault("import-bad-kdf").await;
    let backup = service.export_backup().await.expect("export");
    let tampered = mutate_backup(&backup, |document| {
        document["kdf"]["algorithm"] = serde_json::Value::String("pbkdf2".into());
    });
    drop(service);

    let fresh = TempVault::new("import-bad-kdf-target");
    let target = VaultService::open(&fresh.db_path()).await.expect("open");
    let error = target
        .import_backup(&tampered, false)
        .await
        .expect_err("an unsupported KDF must be refused");
    assert!(matches!(error, VaultError::Corrupt(_)));

    // Nothing was written, so the location is still a clean slate.
    assert!(
        !target.status().await.expect("status").exists,
        "a refused import must not leave a half-written vault"
    );
    drop(temp);
    drop(fresh);
}

#[tokio::test]
async fn importing_a_backup_with_a_truncated_wrapped_key_is_refused() {
    let (temp, service) = seeded_vault("import-bad-wrapped-key").await;
    let backup = service.export_backup().await.expect("export");
    // A wrapped key that is not exactly one key plus one tag could never have come
    // from this vault, and storing it would make every future unlock fail.
    let tampered = mutate_backup(&backup, |document| {
        document["wrappedKeyBlob"] = serde_json::Value::String("AAAA".into());
    });
    drop(service);

    let fresh = TempVault::new("import-bad-wrapped-target");
    let target = VaultService::open(&fresh.db_path()).await.expect("open");
    let error = target
        .import_backup(&tampered, false)
        .await
        .expect_err("a truncated wrapped key must be refused");
    assert!(matches!(error, VaultError::Backup(_)));
    assert!(!target.status().await.expect("status").exists);
    drop(temp);
    drop(fresh);
}

#[tokio::test]
async fn importing_a_backup_with_an_unusable_record_is_refused() {
    let (temp, service) = seeded_vault("import-bad-record").await;
    let backup = service.export_backup().await.expect("export");
    let tampered = mutate_backup(&backup, |document| {
        // A nonce that is not 192 bits cannot have been produced by this vault.
        document["items"][0]["nonce"] = serde_json::Value::String("AAAA".into());
    });
    drop(service);

    let fresh = TempVault::new("import-bad-record-target");
    let target = VaultService::open(&fresh.db_path()).await.expect("open");
    let error = target
        .import_backup(&tampered, false)
        .await
        .expect_err("an unusable record must be refused");
    assert!(matches!(error, VaultError::Backup(_)));
    assert!(!target.status().await.expect("status").exists);
    drop(temp);
    drop(fresh);
}

#[tokio::test]
async fn a_refused_import_can_be_retried_with_a_good_backup() {
    // The point of validating before writing: a failed restore leaves the
    // location usable, so the user can simply pick the right file.
    let (temp, service) = seeded_vault("import-retry").await;
    let good = service.export_backup().await.expect("export");
    let bad = mutate_backup(&good, |document| {
        document["wrappedKeyNonce"] = serde_json::Value::String("AAAA".into());
    });
    drop(service);

    let fresh = TempVault::new("import-retry-target");
    let target = VaultService::open(&fresh.db_path()).await.expect("open");
    assert!(target.import_backup(&bad, false).await.is_err());

    let summary = target
        .import_backup(&good, false)
        .await
        .expect("a good backup must still be restorable after a refused one");
    assert_eq!(summary.item_count, 1);

    target
        .unlock(master(MASTER_PASSWORD))
        .await
        .expect("the restored vault opens");
    drop(temp);
    drop(fresh);
}

// =====================================================================
// The root secret is pinned, not merely wiped
// =====================================================================

#[tokio::test]
async fn the_master_password_is_held_in_locked_memory() {
    // Wiping on drop does nothing about a copy the operating system already paged
    // out. The master password is the root of the whole trust chain, so it must be
    // pinned for the same reason the vault master key is.
    if !workbench_vault::locked::is_supported() {
        return;
    }
    assert!(
        master(MASTER_PASSWORD).is_memory_locked(),
        "the master password must be pinned out of the page file"
    );
}
