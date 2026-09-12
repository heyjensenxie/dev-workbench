//! Vault data model.
//!
//! The split between [`VaultItemPayload`] and [`VaultItem`] is a security
//! boundary, not a style choice:
//!
//! * [`VaultItemPayload`] is the *entire* sensitive body of a record — title,
//!   username, password, URL, notes, tags, custom fields. It is serialised to
//!   JSON and immediately AEAD-encrypted. It is the only shape allowed to reach
//!   storage, and it is wiped on drop.
//! * [`VaultItem`] is the decrypted record returned to the UI. It is only ever
//!   produced in memory, only for an unlocked vault, and it is wiped on drop.
//! * [`VaultItemSummary`] is the *list* projection. It deliberately omits the
//!   password, notes, and custom field values, so browsing the vault does not
//!   push every secret into the WebView. The UI fetches one full item at a time,
//!   on demand.
//!
//! Even the title and tags are treated as sensitive and kept inside the
//! ciphertext: a plaintext index would be convenient for search, and is exactly
//! what must not exist on disk.

use std::fmt;

use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::error::{VaultError, VaultResult};

/// Version of the vault file/format layout.
pub const VAULT_FORMAT_VERSION: u32 = 1;
/// Version of a single encrypted record's payload schema.
pub const ITEM_RECORD_VERSION: u32 = 1;

/// Maximum accepted length for the free-text fields, to bound memory use.
const MAX_TEXT_LEN: usize = 16 * 1024;
const MAX_TAGS: usize = 64;
const MAX_FIELDS: usize = 64;

/// One custom field on an item.
#[derive(Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(rename_all = "camelCase")]
pub struct VaultItemField {
    pub name: String,
    pub value: String,
    #[serde(default)]
    pub sensitive: bool,
}

impl fmt::Debug for VaultItemField {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The value is redacted unconditionally: whether a field is flagged
        // sensitive should not decide whether a stray log line leaks it.
        formatter
            .debug_struct("VaultItemField")
            .field("name", &self.name)
            .field("value", &"<redacted>")
            .field("sensitive", &self.sensitive)
            .finish()
    }
}

/// The sensitive body of a vault record: everything that is encrypted at rest.
#[derive(Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(rename_all = "camelCase")]
pub struct VaultItemPayload {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub custom_fields: Vec<VaultItemField>,
    #[serde(default)]
    pub favorite: bool,
}

impl fmt::Debug for VaultItemPayload {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("VaultItemPayload")
            .field("title", &self.title)
            .field("username", &self.username.as_ref().map(|_| "<redacted>"))
            .field("password", &self.password.as_ref().map(|_| "<redacted>"))
            .field("url", &self.url)
            .field("notes", &self.notes.as_ref().map(|_| "<redacted>"))
            .field("tags", &self.tags)
            .field("customFieldCount", &self.custom_fields.len())
            .field("favorite", &self.favorite)
            .finish()
    }
}

impl VaultItemPayload {
    /// Normalises and validates user input before it is ever encrypted.
    pub fn normalize(mut self) -> VaultResult<Self> {
        self.title = self.title.trim().to_owned();
        if self.title.is_empty() {
            return Err(VaultError::Validation("title must not be empty".into()));
        }
        if self.title.chars().count() > 200 {
            return Err(VaultError::Validation(
                "title must be 200 characters or fewer".into(),
            ));
        }
        for (label, value) in [
            ("username", &self.username),
            ("password", &self.password),
            ("url", &self.url),
            ("notes", &self.notes),
        ] {
            if value.as_ref().is_some_and(|text| text.len() > MAX_TEXT_LEN) {
                return Err(VaultError::Validation(format!("{label} is too long")));
            }
        }
        let tags = self
            .tags
            .iter()
            .map(|tag| tag.trim().to_owned())
            .filter(|tag| !tag.is_empty())
            .take(MAX_TAGS)
            .collect::<Vec<_>>();
        self.tags = tags;
        self.tags.dedup();
        if self.custom_fields.len() > MAX_FIELDS {
            return Err(VaultError::Validation("too many custom fields".into()));
        }
        for field in &self.custom_fields {
            if field.name.trim().is_empty() {
                return Err(VaultError::Validation(
                    "custom field name must not be empty".into(),
                ));
            }
            if field.value.len() > MAX_TEXT_LEN {
                return Err(VaultError::Validation(
                    "custom field value is too long".into(),
                ));
            }
        }
        Ok(self)
    }
}

/// A decrypted vault record, as handed to the UI one item at a time.
///
/// **Invariant: a `VaultItem` never carries secret values out of the vault
/// service.** Opening an item reveals its title, username, URL, tags, notes, and
/// non-sensitive custom fields — everything the detail pane displays — but the
/// password and any field marked sensitive are stripped by
/// [`VaultItem::without_secrets`] before serialisation. A secret reaches the UI
/// only when the user explicitly reveals or copies that one field, through
/// [`crate::VaultService::reveal_field`].
///
/// `has_password` records whether a password existed *before* it was stripped, so
/// the UI can still offer a Reveal control.
#[derive(Serialize, Zeroize, ZeroizeOnDrop)]
#[serde(rename_all = "camelCase")]
pub struct VaultItem {
    pub id: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    pub tags: Vec<String>,
    pub custom_fields: Vec<VaultItemField>,
    pub favorite: bool,
    /// Whether a password is stored for this item. Survives redaction so the UI
    /// can show a masked field and a Reveal control without holding the value.
    pub has_password: bool,
}

impl VaultItem {
    /// Moves the payload's fields into a full item.
    ///
    /// Uses `mem::take` rather than destructuring because `VaultItemPayload`
    /// implements `Drop` (to wipe itself), which forbids partial moves out of it.
    pub fn from_parts(
        id: String,
        created_at: i64,
        updated_at: i64,
        mut payload: VaultItemPayload,
    ) -> Self {
        let has_password = payload
            .password
            .as_ref()
            .is_some_and(|value| !value.is_empty());
        Self {
            id,
            created_at,
            updated_at,
            title: std::mem::take(&mut payload.title),
            username: payload.username.take(),
            password: payload.password.take(),
            url: payload.url.take(),
            notes: payload.notes.take(),
            tags: std::mem::take(&mut payload.tags),
            custom_fields: std::mem::take(&mut payload.custom_fields),
            favorite: payload.favorite,
            has_password,
        }
    }

    /// Strips every secret value, leaving only what the detail pane displays.
    ///
    /// Each removed value is overwritten before its allocation is released: this
    /// struct wipes its own inline fields on drop, but reassigning a `String`
    /// field would free the old buffer without clearing it.
    pub fn without_secrets(mut self) -> Self {
        if let Some(mut password) = self.password.take() {
            password.zeroize();
        }
        for field in &mut self.custom_fields {
            if field.sensitive {
                // `Zeroize` on a `String` clears the bytes and the length, so the
                // buffer is empty by the time the allocator reclaims it.
                field.value.zeroize();
            }
        }
        self
    }

    /// Projects the item down to what a list view and in-memory search need.
    pub fn summary(&self) -> VaultItemSummary {
        VaultItemSummary {
            id: self.id.clone(),
            title: self.title.clone(),
            username: self.username.clone(),
            url: self.url.clone(),
            tags: self.tags.clone(),
            favorite: self.favorite,
            has_password: self.has_password,
            has_notes: self.notes.as_ref().is_some_and(|value| !value.is_empty()),
            custom_field_count: self.custom_fields.len(),
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

impl fmt::Debug for VaultItem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("VaultItem")
            .field("id", &self.id)
            .field("title", &self.title)
            .field("createdAt", &self.created_at)
            .field("updatedAt", &self.updated_at)
            .field("hasPassword", &self.password.is_some())
            .field("fieldCount", &self.custom_fields.len())
            .finish_non_exhaustive()
    }
}

/// The list projection of a vault item. Carries no password, notes, or custom
/// field values.
#[derive(Serialize, Clone, Zeroize, ZeroizeOnDrop)]
#[serde(rename_all = "camelCase")]
pub struct VaultItemSummary {
    pub id: String,
    pub title: String,
    pub username: Option<String>,
    pub url: Option<String>,
    pub tags: Vec<String>,
    pub favorite: bool,
    pub has_password: bool,
    pub has_notes: bool,
    pub custom_field_count: usize,
    pub created_at: i64,
    pub updated_at: i64,
}

impl fmt::Debug for VaultItemSummary {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("VaultItemSummary")
            .field("id", &self.id)
            .field("title", &self.title)
            .finish_non_exhaustive()
    }
}

/// Public state of the vault. Contains no secret material.
#[derive(Serialize, Clone, Copy, Debug)]
#[serde(rename_all = "camelCase")]
pub struct VaultStatus {
    pub exists: bool,
    pub unlocked: bool,
    pub format_version: u32,
    pub item_count: i64,
    pub auto_lock_seconds: u64,
    /// Whether the live vault master key is pinned out of the page file. `false`
    /// while locked, and on a platform where locking is unavailable, so the UI
    /// never claims protection that is not in force.
    pub memory_locked: bool,
    /// Consecutive failed unlock attempts. Reset only by a successful unlock.
    pub failed_attempts: u32,
    /// When an unlock lockout expires, in milliseconds since the Unix epoch, or
    /// `None` while no lockout is in force. The UI counts down from this rather
    /// than polling.
    pub locked_until: Option<i64>,
    pub kdf: Option<VaultKdfSummary>,
}

/// Non-secret KDF facts, surfaced so the UI can explain the cost of unlocking.
#[derive(Serialize, Clone, Copy, Debug)]
#[serde(rename_all = "camelCase")]
pub struct VaultKdfSummary {
    pub algorithm: &'static str,
    pub version: u32,
    pub memory_kib: u32,
    pub time_cost: u32,
    pub parallelism: u32,
    /// Whether these parameters meet the current security floor. A vault created
    /// by an earlier build can legitimately report `false`; the UI says so, and
    /// changing the master password upgrades it.
    pub meets_current_floor: bool,
}

/// Result of an encrypted backup export.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct VaultBackupSummary {
    pub item_count: usize,
    pub format_version: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload() -> VaultItemPayload {
        VaultItemPayload {
            title: "GitHub".into(),
            username: Some("jensen@example.com".into()),
            password: Some("correct-horse-battery-staple".into()),
            url: Some("https://github.com".into()),
            notes: Some("recovery codes are elsewhere".into()),
            tags: vec!["work".into()],
            custom_fields: vec![VaultItemField {
                name: "org".into(),
                value: "acme".into(),
                sensitive: false,
            }],
            favorite: true,
        }
    }

    #[test]
    fn payload_debug_never_prints_secrets() {
        let rendered = format!("{:?}", payload());
        assert!(!rendered.contains("correct-horse-battery-staple"));
        assert!(!rendered.contains("jensen@example.com"));
        assert!(!rendered.contains("recovery codes"));
        assert!(!rendered.contains("acme"));
        assert!(rendered.contains("GitHub"), "the title stays for debugging");
    }

    #[test]
    fn custom_field_debug_redacts_even_non_sensitive_values() {
        let rendered = format!(
            "{:?}",
            VaultItemField {
                name: "org".into(),
                value: "acme".into(),
                sensitive: false,
            }
        );
        assert!(!rendered.contains("acme"));
    }

    #[test]
    fn summary_omits_every_secret_field() {
        let item = VaultItem::from_parts("id-1".into(), 0, 1, payload());
        let encoded = serde_json::to_string(&item.summary()).expect("serialize");
        assert!(!encoded.contains("correct-horse-battery-staple"));
        assert!(!encoded.contains("recovery codes"));
        assert!(!encoded.contains("acme"));
        assert!(encoded.contains("\"hasPassword\":true"));
    }

    #[test]
    fn normalize_rejects_an_empty_title() {
        let mut candidate = payload();
        candidate.title = "   ".into();
        assert!(candidate.normalize().is_err());
    }

    #[test]
    fn normalize_trims_and_deduplicates_tags() {
        let mut candidate = payload();
        candidate.tags = vec![" work ".into(), "work".into(), "".into(), " personal ".into()];
        let normalized = candidate.normalize().expect("normalize");
        assert_eq!(normalized.tags, vec!["work", "personal"]);
    }

    #[test]
    fn normalize_rejects_an_empty_custom_field_name() {
        let mut candidate = payload();
        candidate.custom_fields[0].name = "  ".into();
        assert!(candidate.normalize().is_err());
    }
}
