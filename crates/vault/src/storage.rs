//! Ciphertext-only persistence.
//!
//! The vault lives in its own SQLite file (`vault.db`), never in the ordinary
//! `workbench.sqlite3` database. The reason is not tidiness: the workspace
//! database is read and written by unrelated features, so a password stored
//! there would sit inside the blast radius of every future query, export,
//! migration, and debugging tool in the app. A separate file with a separate
//! schema means nothing else can reach the ciphertext by accident.
//!
//! The invariant this module enforces is that **whatever crosses into SQLite is
//! already ciphertext**. Read the schema below and note what is absent: there is
//! no `title`, `username`, `password`, `url`, or `notes` column anywhere. Only
//! an opaque blob, its nonce, and non-secret bookkeeping (an opaque random id
//! and timestamps) are stored.
//!
//! That property is what makes the surrounding storage concerns safe rather
//! than lucky:
//!
//! * A write-ahead log can only ever contain ciphertext rows.
//! * Temporary files and shared-memory files can only ever contain ciphertext.
//! * `PRAGMA secure_delete` is enabled as good hygiene, but it is *not* what
//!   protects the data — see the module docs for [`crate::VaultService`], which
//!   deliberately makes no claim of physical erasure on SSDs or journaling
//!   filesystems.
//! * File permissions are not relied upon either. An attacker who copies
//!   `vault.db` off the machine gets exactly the same thing as one who reads it
//!   in place: ciphertext they cannot open without the master password.

use std::path::Path;
use std::time::Duration;

use sqlx::sqlite::{
    SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous,
};
use sqlx::{Row, SqlitePool};

use crate::error::{VaultError, VaultResult};
use crate::key::WrappedKey;
use crate::kdf::KdfParams;

/// One encrypted record as it exists on disk. Field names here mirror the
/// physical columns; none of them is a secret.
#[derive(Clone)]
pub struct StoredRecord {
    pub id: String,
    pub nonce: Vec<u8>,
    pub ciphertext: Vec<u8>,
    pub record_version: u32,
    pub created_at: i64,
    pub updated_at: i64,
}

/// The vault header. Everything here is safe to read before unlocking: it is
/// the public description of how to derive the key, plus the wrapped key.
#[derive(Clone)]
pub struct VaultMeta {
    pub format_version: u32,
    pub kdf: KdfParams,
    pub wrapped_key: WrappedKey,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Schema for `vault.db`. Applied once; tracked in `vault_migrations`.
const MIGRATIONS: &[(&str, &str)] = &[
    (
        "0001_vault_format_1",
        r#"
    CREATE TABLE vault_meta (
        id                   INTEGER PRIMARY KEY CHECK (id = 1),
        format_version       INTEGER NOT NULL,
        kdf_algorithm        TEXT    NOT NULL,
        kdf_version          INTEGER NOT NULL,
        kdf_salt             BLOB    NOT NULL,
        kdf_memory_kib       INTEGER NOT NULL,
        kdf_time_cost        INTEGER NOT NULL,
        kdf_parallelism      INTEGER NOT NULL,
        kdf_output_len       INTEGER NOT NULL,
        wrapped_key_nonce    BLOB    NOT NULL,
        wrapped_key_blob     BLOB    NOT NULL,
        created_at           INTEGER NOT NULL,
        updated_at           INTEGER NOT NULL
    );

    CREATE TABLE vault_items (
        id             TEXT    PRIMARY KEY NOT NULL,
        nonce          BLOB    NOT NULL,
        ciphertext     BLOB    NOT NULL,
        record_version INTEGER NOT NULL,
        created_at     INTEGER NOT NULL,
        updated_at     INTEGER NOT NULL
    );
    "#,
    ),
    // Unlock throttling. These two columns are the only mutable state outside the
    // encrypted records, and neither is secret: a failure count and a timestamp.
    // They live in the header so a lockout survives closing the application —
    // throttling that could be cleared by quitting and relaunching would not be
    // throttling at all.
    (
        "0002_unlock_throttling",
        r#"
    ALTER TABLE vault_meta ADD COLUMN failed_attempts INTEGER NOT NULL DEFAULT 0;
    ALTER TABLE vault_meta ADD COLUMN locked_until INTEGER;
    "#,
    ),
];

/// Consecutive failed unlock attempts, and when the current lockout expires.
///
/// Recorded in the vault header in plaintext. An attacker with file access can
/// reset it — see the module docs for [`crate::lockout`] — which is exactly why
/// this is described as an interactive throttle and not as attack protection.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UnlockThrottle {
    pub failed_attempts: u32,
    /// Milliseconds since the Unix epoch, or `None` when not locked out.
    pub locked_until: Option<i64>,
}


pub struct VaultStorage {
    pool: SqlitePool,
}

impl VaultStorage {
    /// Opens (creating if absent) the vault database and applies migrations.
    pub async fn open(path: &Path) -> VaultResult<Self> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            // Overwrite deleted content in the file's free pages. Ciphertext
            // only, so this is belt-and-braces rather than the security basis.
            .pragma("secure_delete", "ON")
            // `vault_items.id` is the rowid key; foreign keys are not used.
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .busy_timeout(Duration::from_secs(5));
        // A single writer keeps the vault's transaction story simple: password
        // changes and key rotation must be all-or-nothing.
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .map_err(VaultError::Storage)?;
        let storage = Self { pool };
        storage.migrate().await?;
        Ok(storage)
    }

    async fn migrate(&self) -> VaultResult<()> {
        sqlx::raw_sql(
            "CREATE TABLE IF NOT EXISTS vault_migrations (name TEXT PRIMARY KEY NOT NULL, applied_at INTEGER NOT NULL);",
        )
        .execute(&self.pool)
        .await
        .map_err(VaultError::Storage)?;

        let applied: Vec<String> = sqlx::query_scalar("SELECT name FROM vault_migrations")
            .fetch_all(&self.pool)
            .await
            .map_err(VaultError::Storage)?;

        for (name, sql) in MIGRATIONS {
            if applied.iter().any(|entry| entry == name) {
                continue;
            }
            let mut transaction = self.pool.begin().await.map_err(VaultError::Storage)?;
            sqlx::raw_sql(sql)
                .execute(&mut *transaction)
                .await
                .map_err(VaultError::Storage)?;
            sqlx::query("INSERT INTO vault_migrations(name, applied_at) VALUES(?,?)")
                .bind(name)
                .bind(crate::now_millis())
                .execute(&mut *transaction)
                .await
                .map_err(VaultError::Storage)?;
            transaction.commit().await.map_err(VaultError::Storage)?;
        }
        Ok(())
    }

    pub async fn is_initialized(&self) -> VaultResult<bool> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM vault_meta")
            .fetch_one(&self.pool)
            .await
            .map_err(VaultError::Storage)?;
        Ok(count > 0)
    }

    pub async fn load_meta(&self) -> VaultResult<Option<VaultMeta>> {
        let row = sqlx::query("SELECT format_version,kdf_algorithm,kdf_version,kdf_salt,kdf_memory_kib,kdf_time_cost,kdf_parallelism,kdf_output_len,wrapped_key_nonce,wrapped_key_blob,created_at,updated_at FROM vault_meta WHERE id=1")
            .fetch_optional(&self.pool)
            .await
            .map_err(VaultError::Storage)?;
        row.as_ref().map(meta_from_row).transpose()
    }

    /// Writes the header for a brand-new vault. Fails if one already exists, so
    /// a second `create` can never silently overwrite key material.
    pub async fn insert_meta(&self, meta: &VaultMeta) -> VaultResult<()> {
        sqlx::query("INSERT INTO vault_meta(id,format_version,kdf_algorithm,kdf_version,kdf_salt,kdf_memory_kib,kdf_time_cost,kdf_parallelism,kdf_output_len,wrapped_key_nonce,wrapped_key_blob,created_at,updated_at) VALUES(1,?,?,?,?,?,?,?,?,?,?,?,?)")
            .bind(meta.format_version as i64)
            .bind(&meta.kdf.algorithm)
            .bind(meta.kdf.version as i64)
            .bind(&meta.kdf.salt)
            .bind(meta.kdf.memory_kib as i64)
            .bind(meta.kdf.time_cost as i64)
            .bind(meta.kdf.parallelism as i64)
            .bind(meta.kdf.output_len as i64)
            .bind(&meta.wrapped_key.nonce)
            .bind(&meta.wrapped_key.ciphertext)
            .bind(meta.created_at)
            .bind(meta.updated_at)
            .execute(&self.pool)
            .await
            .map_err(VaultError::Storage)?;
        Ok(())
    }

    pub async fn list_records(&self) -> VaultResult<Vec<StoredRecord>> {
        let rows = sqlx::query(
            "SELECT id,nonce,ciphertext,record_version,created_at,updated_at FROM vault_items ORDER BY updated_at DESC, id ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(VaultError::Storage)?;
        rows.iter().map(record_from_row).collect()
    }

    pub async fn get_record(&self, id: &str) -> VaultResult<Option<StoredRecord>> {
        let row = sqlx::query(
            "SELECT id,nonce,ciphertext,record_version,created_at,updated_at FROM vault_items WHERE id=?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(VaultError::Storage)?;
        row.as_ref().map(record_from_row).transpose()
    }

    pub async fn put_record(&self, record: &StoredRecord) -> VaultResult<()> {
        let mut transaction = self.pool.begin().await.map_err(VaultError::Storage)?;
        sqlx::query("INSERT INTO vault_items(id,nonce,ciphertext,record_version,created_at,updated_at) VALUES(?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET nonce=excluded.nonce,ciphertext=excluded.ciphertext,record_version=excluded.record_version,updated_at=excluded.updated_at")
            .bind(&record.id)
            .bind(&record.nonce)
            .bind(&record.ciphertext)
            .bind(record.record_version as i64)
            .bind(record.created_at)
            .bind(record.updated_at)
            .execute(&mut *transaction)
            .await
            .map_err(VaultError::Storage)?;
        transaction.commit().await.map_err(VaultError::Storage)?;
        Ok(())
    }

    pub async fn delete_record(&self, id: &str) -> VaultResult<bool> {
        let result = sqlx::query("DELETE FROM vault_items WHERE id=?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(VaultError::Storage)?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn count_records(&self) -> VaultResult<i64> {
        sqlx::query_scalar("SELECT COUNT(*) FROM vault_items")
            .fetch_one(&self.pool)
            .await
            .map_err(VaultError::Storage)
    }

    /// Reads the unlock throttle.
    pub async fn load_throttle(&self) -> VaultResult<UnlockThrottle> {
        let row = sqlx::query("SELECT failed_attempts, locked_until FROM vault_meta WHERE id=1")
            .fetch_optional(&self.pool)
            .await
            .map_err(VaultError::Storage)?;
        let Some(row) = row else {
            return Ok(UnlockThrottle::default());
        };
        Ok(UnlockThrottle {
            failed_attempts: as_u32(
                row.try_get::<i64, _>("failed_attempts")
                    .map_err(VaultError::Storage)?,
            ),
            locked_until: row.try_get("locked_until").map_err(VaultError::Storage)?,
        })
    }

    /// Records a failed attempt and, when the policy calls for it, the resulting
    /// lockout.
    pub async fn record_failed_attempt(
        &self,
        failed_attempts: u32,
        locked_until: Option<i64>,
    ) -> VaultResult<()> {
        sqlx::query("UPDATE vault_meta SET failed_attempts=?, locked_until=? WHERE id=1")
            .bind(failed_attempts as i64)
            .bind(locked_until)
            .execute(&self.pool)
            .await
            .map_err(VaultError::Storage)?;
        Ok(())
    }

    /// Clears the throttle after a successful unlock.
    ///
    /// Only a correct password resets this. An expired lockout does not, which is
    /// what makes a later failure escalate rather than start over.
    pub async fn clear_throttle(&self) -> VaultResult<()> {
        sqlx::query("UPDATE vault_meta SET failed_attempts=0, locked_until=NULL WHERE id=1")
            .execute(&self.pool)
            .await
            .map_err(VaultError::Storage)?;
        Ok(())
    }

    /// Removes every trace of the vault: the header, the wrapped key, and all
    /// records.    ///
    /// Three steps, because deleting rows alone is not enough:
    ///
    /// 1. Delete everything in one transaction, so the vault is either wholly
    ///    present or wholly gone.
    /// 2. Checkpoint the write-ahead log, folding it into the main file. Without
    ///    this the removed ciphertext would still sit in the `-wal` sidecar.
    /// 3. `VACUUM` to rebuild the file from its live pages, so the removed
    ///    content is not merely on the free list.
    ///
    /// `secure_delete` is enabled on the connection, so deleted pages are zeroed
    /// as they are freed. None of this is a physical-erasure guarantee — see the
    /// crate documentation — but it does remove the vault from everything SQLite
    /// can reach.
    ///
    /// The database file itself remains, ready to host a new vault. It is left in
    /// place deliberately: the file is the schema, and recreating it here would
    /// require tearing down the live connection.
    pub async fn erase_all(&self) -> VaultResult<()> {
        let mut transaction = self.pool.begin().await.map_err(VaultError::Storage)?;
        sqlx::query("DELETE FROM vault_items")
            .execute(&mut *transaction)
            .await
            .map_err(VaultError::Storage)?;
        sqlx::query("DELETE FROM vault_meta")
            .execute(&mut *transaction)
            .await
            .map_err(VaultError::Storage)?;
        transaction.commit().await.map_err(VaultError::Storage)?;

        // `PRAGMA wal_checkpoint` reports a row, so it is fetched rather than
        // executed. `TRUNCATE` also shrinks the log to zero bytes.
        sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
            .fetch_optional(&self.pool)
            .await
            .map_err(VaultError::Storage)?;
        sqlx::raw_sql("VACUUM;")
            .execute(&self.pool)
            .await
            .map_err(VaultError::Storage)?;
        Ok(())
    }

    /// Rewrites only the header, leaving records untouched.
    ///
    /// This is the master-password change path: the vault master key is
    /// unchanged, so only the wrapper around it needs to move. Re-encrypting
    /// every record would be pointless work, and doing it here would risk
    /// rewriting ciphertext that never needed to change.
    pub async fn update_meta(&self, meta: &VaultMeta) -> VaultResult<()> {
        sqlx::query("UPDATE vault_meta SET format_version=?,kdf_algorithm=?,kdf_version=?,kdf_salt=?,kdf_memory_kib=?,kdf_time_cost=?,kdf_parallelism=?,kdf_output_len=?,wrapped_key_nonce=?,wrapped_key_blob=?,updated_at=? WHERE id=1")
            .bind(meta.format_version as i64)
            .bind(&meta.kdf.algorithm)
            .bind(meta.kdf.version as i64)
            .bind(&meta.kdf.salt)
            .bind(meta.kdf.memory_kib as i64)
            .bind(meta.kdf.time_cost as i64)
            .bind(meta.kdf.parallelism as i64)
            .bind(meta.kdf.output_len as i64)
            .bind(&meta.wrapped_key.nonce)
            .bind(&meta.wrapped_key.ciphertext)
            .bind(meta.updated_at)
            .execute(&self.pool)
            .await
            .map_err(VaultError::Storage)?;
        Ok(())
    }

    /// Replaces the header and every record in one transaction.
    ///
    /// Used by master-password changes and by vault-key rotation, both of which
    /// must never be observable half-applied: a crash midway has to leave either
    /// the old key or the new key consistently in force.
    pub async fn replace_all(
        &self,
        meta: &VaultMeta,
        records: &[StoredRecord],
    ) -> VaultResult<()> {
        let mut transaction = self.pool.begin().await.map_err(VaultError::Storage)?;

        sqlx::query("DELETE FROM vault_items")
            .execute(&mut *transaction)
            .await
            .map_err(VaultError::Storage)?;
        for record in records {
            sqlx::query("INSERT INTO vault_items(id,nonce,ciphertext,record_version,created_at,updated_at) VALUES(?,?,?,?,?,?)")
                .bind(&record.id)
                .bind(&record.nonce)
                .bind(&record.ciphertext)
                .bind(record.record_version as i64)
                .bind(record.created_at)
                .bind(record.updated_at)
                .execute(&mut *transaction)
                .await
                .map_err(VaultError::Storage)?;
        }

        // Upsert rather than update, so the same call also seeds the header when
        // restoring a backup into an empty file.
        sqlx::query("INSERT INTO vault_meta(id,format_version,kdf_algorithm,kdf_version,kdf_salt,kdf_memory_kib,kdf_time_cost,kdf_parallelism,kdf_output_len,wrapped_key_nonce,wrapped_key_blob,created_at,updated_at) VALUES(1,?,?,?,?,?,?,?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET format_version=excluded.format_version,kdf_algorithm=excluded.kdf_algorithm,kdf_version=excluded.kdf_version,kdf_salt=excluded.kdf_salt,kdf_memory_kib=excluded.kdf_memory_kib,kdf_time_cost=excluded.kdf_time_cost,kdf_parallelism=excluded.kdf_parallelism,kdf_output_len=excluded.kdf_output_len,wrapped_key_nonce=excluded.wrapped_key_nonce,wrapped_key_blob=excluded.wrapped_key_blob,created_at=excluded.created_at,updated_at=excluded.updated_at")
            .bind(meta.format_version as i64)
            .bind(&meta.kdf.algorithm)
            .bind(meta.kdf.version as i64)
            .bind(&meta.kdf.salt)
            .bind(meta.kdf.memory_kib as i64)
            .bind(meta.kdf.time_cost as i64)
            .bind(meta.kdf.parallelism as i64)
            .bind(meta.kdf.output_len as i64)
            .bind(&meta.wrapped_key.nonce)
            .bind(&meta.wrapped_key.ciphertext)
            .bind(meta.created_at)
            .bind(meta.updated_at)
            .execute(&mut *transaction)
            .await
            .map_err(VaultError::Storage)?;

        transaction.commit().await.map_err(VaultError::Storage)?;
        Ok(())
    }
}

fn meta_from_row(row: &sqlx::sqlite::SqliteRow) -> VaultResult<VaultMeta> {
    Ok(VaultMeta {
        format_version: as_u32(row.try_get::<i64, _>("format_version").map_err(VaultError::Storage)?),
        kdf: KdfParams {
            algorithm: row.try_get("kdf_algorithm").map_err(VaultError::Storage)?,
            version: as_u32(row.try_get::<i64, _>("kdf_version").map_err(VaultError::Storage)?),
            salt: row.try_get("kdf_salt").map_err(VaultError::Storage)?,
            memory_kib: as_u32(row.try_get::<i64, _>("kdf_memory_kib").map_err(VaultError::Storage)?),
            time_cost: as_u32(row.try_get::<i64, _>("kdf_time_cost").map_err(VaultError::Storage)?),
            parallelism: as_u32(row.try_get::<i64, _>("kdf_parallelism").map_err(VaultError::Storage)?),
            output_len: row
                .try_get::<i64, _>("kdf_output_len")
                .map_err(VaultError::Storage)? as usize,
        },
        wrapped_key: WrappedKey {
            nonce: row.try_get("wrapped_key_nonce").map_err(VaultError::Storage)?,
            ciphertext: row.try_get("wrapped_key_blob").map_err(VaultError::Storage)?,
        },
        created_at: row.try_get("created_at").map_err(VaultError::Storage)?,
        updated_at: row.try_get("updated_at").map_err(VaultError::Storage)?,
    })
}

fn record_from_row(row: &sqlx::sqlite::SqliteRow) -> VaultResult<StoredRecord> {
    Ok(StoredRecord {
        id: row.try_get("id").map_err(VaultError::Storage)?,
        nonce: row.try_get("nonce").map_err(VaultError::Storage)?,
        ciphertext: row.try_get("ciphertext").map_err(VaultError::Storage)?,
        record_version: as_u32(
            row.try_get::<i64, _>("record_version")
                .map_err(VaultError::Storage)?,
        ),
        created_at: row.try_get("created_at").map_err(VaultError::Storage)?,
        updated_at: row.try_get("updated_at").map_err(VaultError::Storage)?,
    })
}

fn as_u32(value: i64) -> u32 {
    value.clamp(0, u32::MAX as i64) as u32
}
