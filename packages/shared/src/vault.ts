/**
 * Password Vault contract shared by the UI, the core services, and the native
 * bridge.
 *
 * This module is deliberately free of I/O. Everything here is either a type
 * describing what the native vault boundary accepts and returns, or a pure
 * function that makes a security decision the UI must apply consistently.
 *
 * Two rules govern what may appear in this file:
 *
 * 1. No secret ever gets a place to live. There is no vault key type, no
 *    "remember unlocked" flag, and nothing that could be persisted. A decrypted
 *    item exists only as a `VaultItem` returned by a single explicit read.
 * 2. Security decisions live here rather than in templates, so they can be
 *    unit-tested: URL scheme allow-listing and master-password scoring are both
 *    plain functions with no rendering attached.
 */

// ---------------------------------------------------------------- constants

/**
 * Shortest accepted master password. Length is the property that matters; the
 * vault deliberately does not require upper/lower/digit/symbol combinations,
 * because those rules push users toward short, hard-to-remember passwords while
 * a long passphrase is stronger and easier to recall.
 */
export const MASTER_PASSWORD_MIN_LENGTH = 12
/** Length at which the strength meter stops asking for more. */
export const MASTER_PASSWORD_RECOMMENDED_LENGTH = 16

/** Auto-lock choices offered in Settings, in seconds ("Never" is not offered). */
export const VAULT_AUTO_LOCK_OPTIONS = [60, 300, 600, 1800] as const
/** Default idle timeout: five minutes. */
export const DEFAULT_VAULT_AUTO_LOCK_SECONDS = 300
/** How long the application may stay in the background before the vault locks. */
export const VAULT_BACKGROUND_LOCK_SECONDS = 60

/** Clipboard auto-clear choices for copied passwords, in seconds. */
export const VAULT_CLIPBOARD_CLEAR_OPTIONS = [15, 30, 60] as const
/** Default clipboard auto-clear delay. */
export const DEFAULT_VAULT_CLIPBOARD_CLEAR_SECONDS = 30

/** How long a revealed password stays visible before re-hiding itself. */
export const VAULT_REVEAL_SECONDS = 20

/** Schemes an item URL may be opened with. Everything else is refused. */
export const VAULT_ALLOWED_URL_SCHEMES = ['http', 'https'] as const

/** Default shape of a generated password. */
export const VAULT_GENERATOR_DEFAULTS: VaultGeneratorOptions = {  length: 20,
  uppercase: true,
  lowercase: true,
  numbers: true,
  symbols: true,
  excludeAmbiguous: true,
}

export const VAULT_GENERATOR_MIN_LENGTH = 8
export const VAULT_GENERATOR_MAX_LENGTH = 128

/**
 * Memory cost, in MiB, that a newly created vault is guaranteed to meet.
 *
 * Mirrors the floor enforced in `crates/vault/src/kdf.rs`. It is a *floor*, not a
 * target: calibration raises the cost on a fast machine. It is repeated here only
 * so the UI can report a below-floor vault against the same number the native
 * layer enforces.
 */
export const VAULT_KDF_FLOOR_MIB = 64

/**
 * Word the user must type to remove the local vault.
 *
 * Removing a local vault cannot be protected by the master password — anyone with
 * access to the machine can delete the file directly — and requiring it would also
 * block the case that needs this most: a vault that cannot be unlocked. So the
 * guard is deliberate friction instead: the user must type this exactly.
 *
 * Deliberately not localized: a confirmation token that differs by interface
 * language is one more way to mistype it.
 */
export const VAULT_DESTROY_CONFIRMATION = 'REMOVE'

/**
 * Consecutive failed unlocks before the vault locks out and the recovery actions
 * appear.
 *
 * Mirrors `LOCKOUT_THRESHOLD` in `crates/vault/src/lockout.rs`. The native layer
 * decides the policy; this constant exists only so the UI reveals the way out at
 * exactly the moment the lockout begins, instead of the two drifting apart.
 */
export const VAULT_LOCKOUT_THRESHOLD = 5

// -------------------------------------------------------------------- types

export interface VaultKdfSummary {
  algorithm: string
  version: number
  memoryKib: number
  timeCost: number
  parallelism: number
  /**
   * Whether the vault's parameters meet the current security floor.
   *
   * A vault created by an earlier build can legitimately report `false`: its
   * parameters live in its own header and are never rewritten behind the user's
   * back. The UI must say so rather than implying every vault is equally strong,
   * and changing the master password upgrades it.
   */
  meetsCurrentFloor: boolean
}

/** Public vault state. Carries no secret material. */
export interface VaultStatus {
  exists: boolean
  unlocked: boolean
  formatVersion: number
  itemCount: number
  autoLockSeconds: number
  /**
   * Whether the live vault master key is pinned out of the system page file.
   * `false` while locked, and on a platform where locking is unavailable, so the
   * UI never claims protection that is not actually in force.
   */
  memoryLocked: boolean
  /**
   * Consecutive failed unlock attempts. Reset only by a successful unlock, so an
   * expired lockout still escalates the next failure.
   */
  failedAttempts: number
  /**
   * When the current unlock lockout expires, in milliseconds since the Unix epoch,
   * or `null` while no lockout is in force. The UI counts down from this rather
   * than polling the native layer.
   */
  lockedUntil: number | null
  kdf?: VaultKdfSummary
}

export interface VaultItemField {
  name: string
  value: string
  /** Sensitive fields are masked in the UI by default. */
  sensitive: boolean
}

/** The writable body of an item, sent to the native layer to be encrypted. */
export interface VaultItemPayload {
  title: string
  username?: string
  password?: string
  url?: string
  notes?: string
  tags: string[]
  customFields: VaultItemField[]
  favorite: boolean
}

/** A fully decrypted item. Only ever fetched one at a time, on demand. */
export interface VaultItem extends VaultItemPayload {
  id: string
  createdAt: number
  updatedAt: number
  /**
   * Whether a password is stored for this item.
   *
   * Present even though `password` never is: the native layer strips the password
   * and every sensitive custom field from a fetched item, so this flag is how the
   * UI knows to offer a Reveal control. A secret arrives only from
   * `revealField`, one field at a time.
   */
  hasPassword: boolean
}

/**
 * The list projection. The native layer never includes a password, notes, or a
 * custom field value here, so browsing the vault does not fill the WebView with
 * secrets.
 */
export interface VaultItemSummary {
  id: string
  title: string
  username?: string
  url?: string
  tags: string[]
  favorite: boolean
  hasPassword: boolean
  hasNotes: boolean
  customFieldCount: number
  createdAt: number
  updatedAt: number
}

export interface VaultBackupSummary {
  itemCount: number
  formatVersion: number
}

/** Result of restoring a backup, including any pre-replace safety copy. */
export interface VaultImportOutcome extends VaultBackupSummary {
  /**
   * Where the previous vault was saved before it was replaced.
   *
   * Absent when there was no vault to replace. Present means the replace is
   * recoverable: restoring that file puts the previous vault back.
   */
  safetyBackup?: string
}

export interface VaultGeneratorOptions {
  length: number
  uppercase: boolean
  lowercase: boolean
  numbers: boolean
  symbols: boolean
  excludeAmbiguous: boolean
}

/** Reason the vault locked itself, as reported by the native supervisor. */
export type VaultLockReason = 'manual' | 'idle-timeout' | 'resumed-from-sleep' | string

// ------------------------------------------------------------- master password

export type VaultPasswordScore = 0 | 1 | 2 | 3 | 4
export type VaultPasswordLabel = 'veryWeak' | 'weak' | 'fair' | 'strong' | 'excellent'

export interface VaultPasswordStrength {
  length: number
  meetsMinimum: boolean
  meetsRecommended: boolean
  score: VaultPasswordScore
  label: VaultPasswordLabel
}

const SCORE_LABELS: Record<VaultPasswordScore, VaultPasswordLabel> = {
  0: 'veryWeak',
  1: 'weak',
  2: 'fair',
  3: 'strong',
  4: 'excellent',
}

/**
 * Estimates master-password strength, purely to advise the user.
 *
 * Length dominates the score. Character-class variety contributes one point, and
 * obvious repetition or keyboard/alphabet runs subtract one: `aaaaaaaaaaaa` is
 * long but is not a passphrase. This is an advisory heuristic, not a measure of
 * entropy, and the UI presents it as guidance rather than a guarantee.
 */
export function scoreMasterPassword(password: string): VaultPasswordStrength {
  const characters = [...password]
  const length = characters.length
  const meetsMinimum = length >= MASTER_PASSWORD_MIN_LENGTH
  const meetsRecommended = length >= MASTER_PASSWORD_RECOMMENDED_LENGTH

  const classes = [/[a-z]/, /[A-Z]/, /[0-9]/, /[^a-zA-Z0-9]/].filter((pattern) =>
    pattern.test(password)).length

  let score = 0
  if (length >= MASTER_PASSWORD_MIN_LENGTH) score += 1
  if (length >= MASTER_PASSWORD_RECOMMENDED_LENGTH) score += 1
  if (length >= 24) score += 1
  if (classes >= 3) score += 1
  if (hasObviousPattern(characters)) score -= 1

  // Anything below the minimum is never presented as acceptable.
  if (!meetsMinimum) score = Math.min(score, 0)
  const clamped = Math.max(0, Math.min(4, score)) as VaultPasswordScore

  return {
    length,
    meetsMinimum,
    meetsRecommended,
    score: clamped,
    label: SCORE_LABELS[clamped],
  }
}

/** Detects repetition and simple runs, which inflate length without adding strength. */
function hasObviousPattern(characters: string[]): boolean {
  if (characters.length < 2) return false
  const unique = new Set(characters)
  if (unique.size <= 2) return true

  // A single repeated character.
  if (characters.every((character) => character === characters[0])) return true

  // A monotonic run over a third or more of the password, forwards or backwards.
  const runLength = Math.max(3, Math.floor(characters.length / 3))
  let forward = 1
  let backward = 1
  for (let index = 1; index < characters.length; index += 1) {
    const step = characters[index]!.charCodeAt(0) - characters[index - 1]!.charCodeAt(0)
    forward = step === 1 ? forward + 1 : 1
    backward = step === -1 ? backward + 1 : 1
    if (forward >= runLength || backward >= runLength) return true
  }
  return false
}

// -------------------------------------------------------------------- URLs

export type VaultUrlIssue =
  | 'empty'
  | 'controlCharacters'
  | 'noScheme'
  | 'unsupportedScheme'

const URL_SCHEME = /^([a-zA-Z][a-zA-Z0-9+.-]*):/

/**
 * Reports why a stored URL may not be opened, or `undefined` when it is safe.
 *
 * The allow-list is `http` and `https` only. `javascript:`, `file:`, `data:`,
 * `vbscript:`, `shell:`, `powershell:`, `cmd:`, and every unknown scheme are
 * refused, so an item's URL field can never become a script-execution or
 * local-file primitive. The native layer applies the same rule independently.
 */
export function vaultUrlIssue(url: string): VaultUrlIssue | undefined {
  const trimmed = url.trim()
  if (!trimmed) return 'empty'
  // eslint-disable-next-line no-control-regex
  if (/[\u0000-\u001f\u007f]/.test(trimmed)) return 'controlCharacters'
  // A Windows drive path is not a URL. It has a colon, but what precedes it is a
  // single letter followed by a path separator, not a scheme.
  if (/^[a-zA-Z]:[\\/]/.test(trimmed)) return 'noScheme'
  const match = URL_SCHEME.exec(trimmed)
  if (!match) return 'noScheme'
  const scheme = match[1]!.toLowerCase()
  return (VAULT_ALLOWED_URL_SCHEMES as readonly string[]).includes(scheme)
    ? undefined
    : 'unsupportedScheme'
}

export function isVaultUrlOpenable(url: string): boolean {
  return vaultUrlIssue(url) === undefined
}

// ------------------------------------------------------------------- search

/**
 * Matches an item against a search query.
 *
 * Search runs entirely in memory, over already-decrypted summaries, and only
 * while the vault is unlocked. There is deliberately no persistent search index:
 * a plaintext index over titles, usernames, or URLs would survive locking and be
 * readable from disk, which is exactly what the vault exists to prevent.
 */
export function matchesVaultQuery(item: VaultItemSummary, query: string): boolean {
  const terms = query.trim().toLocaleLowerCase().split(/\s+/).filter(Boolean)
  if (!terms.length) return true
  const haystack = [item.title, item.username ?? '', item.url ?? '', ...item.tags]
    .join(' ')
    .toLocaleLowerCase()
  return terms.every((term) => haystack.includes(term))
}
