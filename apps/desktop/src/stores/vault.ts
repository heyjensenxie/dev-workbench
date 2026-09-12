/**
 * Password Vault store.
 *
 * This store holds decrypted vault data for exactly as long as the vault is
 * unlocked, and it is written so that "locked" is the only state it can be
 * *stuck* in without losing data. Three rules shape it:
 *
 * 1. **Nothing is persisted.** No Pinia persistence plugin is installed, and the
 *    store deliberately keeps a single full item rather than the whole vault:
 *    `list()` returns the password-free summary projection, and a password only
 *    enters the WebView when the user opens one specific item.
 * 2. **Locking clears everything.** `clearSensitiveState()` drops the item list,
 *    the open item, the reveal flags, the search query, the editor draft, the
 *    generator output, and the clipboard countdown. It is called on manual lock,
 *    on the native auto-lock event, on leaving the screen, and on the window
 *    becoming hidden for too long — so an unlocked screen cannot be revisited
 *    through the back button, a cached component, or stale state.
 * 3. **Secrets leave only through deliberate actions.** Reveal is per item and
 *    re-hides on a timer; copying goes through the native clipboard command,
 *    which schedules its own clear.
 */

import type { VaultGeneratorOptions, VaultItem, VaultItemField, VaultItemPayload, VaultItemSummary, VaultPasswordStrength, VaultStatus } from '@dev-workbench/shared'
import {
  AppError,
  DEFAULT_VAULT_AUTO_LOCK_SECONDS,
  DEFAULT_VAULT_CLIPBOARD_CLEAR_SECONDS,
  VAULT_GENERATOR_DEFAULTS,
  VAULT_LOCKOUT_THRESHOLD,
  VAULT_REVEAL_SECONDS,
  matchesVaultQuery,
  scoreMasterPassword,
} from '@dev-workbench/shared'
import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { workbench } from '../services/workbench'

export type VaultScreen = 'loading' | 'uninitialized' | 'locked' | 'unlocked'

/** A new item draft. Never holds data that has been persisted yet. */
export function emptyVaultDraft(): VaultItemPayload {
  return { title: '', username: '', password: '', url: '', notes: '', tags: [], customFields: [], favorite: false }
}

export const useVaultStore = defineStore('vault', () => {
  const screen = ref<VaultScreen>('loading')
  const items = ref<VaultItemSummary[]>([])
  const query = ref('')
  const selectedId = ref<string>()
  /**
   * The one full item currently open.
   *
   * Carries no secret values: the native layer strips the password and every
   * sensitive custom field before returning it. What the detail pane displays is
   * here; anything masked must be requested through `revealField`.
   */
  const openItem = ref<VaultItem>()
  const revealed = ref(false)
  /**
   * The single secret field currently revealed, if any.
   *
   * Populated only by an explicit reveal, and cleared when the reveal times out,
   * when another item is opened, or when the screen is left.
   */
  const revealedPassword = ref<string>()
  /** Values of sensitive custom fields the user has explicitly revealed. */
  const revealedFieldValues = ref<Record<string, string>>({})
  const draft = ref<VaultItemPayload>(emptyVaultDraft())
  const editingId = ref<string>()
  const busy = ref(false)
  const error = ref<string>()
  const notice = ref<string>()
  /** Where the previous vault was saved before a replace, for the notice. */
  const safetyBackupPath = ref<string>()
  const status = ref<VaultStatus>()

  /** Generated password. Shown only until the user applies or discards it. */
  const generated = ref<string>()
  const generator = ref<VaultGeneratorOptions>({ ...VAULT_GENERATOR_DEFAULTS })
  const clipboardSeconds = ref(DEFAULT_VAULT_CLIPBOARD_CLEAR_SECONDS)
  const clipboardCountdown = ref(0)
  const autoLockSeconds = ref(DEFAULT_VAULT_AUTO_LOCK_SECONDS)

  /** Seconds left on an unlock lockout; zero when the vault accepts attempts. */
  const lockoutRemaining = ref(0)
  /** Whether the recovery actions are offered: the lockout has begun. */
  const recoveryRevealed = computed(
    () => (status.value?.failedAttempts ?? 0) >= VAULT_LOCKOUT_THRESHOLD,
  )
  const revealedFields = ref<Record<string, boolean>>({})

  let revealTimer: ReturnType<typeof setTimeout> | undefined
  let clipboardTimer: ReturnType<typeof setInterval> | undefined
  let lockoutTimer: ReturnType<typeof setInterval> | undefined
  let lastTouch = 0

  const visibleItems = computed(() => items.value.filter((item) => matchesVaultQuery(item, query.value)))
  const favorites = computed(() => visibleItems.value.filter((item) => item.favorite))

  const strengthOf = (password: string): VaultPasswordStrength => scoreMasterPassword(password)

  // ------------------------------------------------------------ sensitive state

  /**
   * Drops every decrypted value this store is holding.
   *
   * Called whenever the vault locks, whenever the screen is left, and whenever
   * the application is hidden for long enough that the user cannot be assumed to
   * be present. After this runs, nothing in the store can reveal a credential.
   */
  function clearSensitiveState(): void {
    if (revealTimer) clearTimeout(revealTimer)
    revealTimer = undefined
    if (clipboardTimer) clearInterval(clipboardTimer)
    clipboardTimer = undefined
    // The lockout countdown is deliberately not touched here. It is not decrypted
    // data: it has to survive locking, and `syncLockout` is its only owner.

    items.value = []
    openItem.value = undefined
    selectedId.value = undefined
    revealed.value = false
    revealedPassword.value = undefined
    revealedFieldValues.value = {}
    revealedFields.value = {}
    draft.value = emptyVaultDraft()
    editingId.value = undefined
    generated.value = undefined
    query.value = ''
    clipboardCountdown.value = 0
    notice.value = undefined
  }

  /** Makes sure a stale reveal is not left on screen when the user looks away. */
  function hideSecrets(): void {
    revealed.value = false
    revealedPassword.value = undefined
    revealedFieldValues.value = {}
    revealedFields.value = {}
    if (revealTimer) clearTimeout(revealTimer)
    revealTimer = undefined
  }

  function reportError(cause: unknown): void {
    error.value = AppError.from(cause).message
  }

  function dismissError(): void {
    error.value = undefined
  }

  function dismissNotice(): void {
    notice.value = undefined
  }

  // ------------------------------------------------------------------ lifecycle

  async function refreshStatus(): Promise<VaultStatus | undefined> {
    try {
      const next = await workbench.vault.status()
      status.value = next
      autoLockSeconds.value = next.autoLockSeconds
      screen.value = !next.exists ? 'uninitialized' : next.unlocked ? 'unlocked' : 'locked'
      if (!next.unlocked) clearSensitiveState()
      syncLockout(next)
      return next
    } catch (cause) {
      reportError(cause)
      return undefined
    }
  }

  /** Loads the vault screen state. Called on mount and after every lock. */
  async function initialize(): Promise<void> {
    busy.value = true
    error.value = undefined
    try {
      const current = await refreshStatus()
      if (current?.unlocked) await refreshItems()
    } catch (cause) {
      reportError(cause)
    } finally {
      busy.value = false
    }
  }

  async function createVault(masterPassword: string): Promise<boolean> {
    busy.value = true
    error.value = undefined
    try {
      const next = await workbench.vault.create(masterPassword)
      status.value = next
      screen.value = 'unlocked'
      await refreshItems()
      return true
    } catch (cause) {
      reportError(cause)
      return false
    } finally {
      busy.value = false
    }
  }

  async function unlock(masterPassword: string): Promise<boolean> {
    busy.value = true
    error.value = undefined
    try {
      const next = await workbench.vault.unlock(masterPassword)
      status.value = next
      screen.value = 'unlocked'
      syncLockout(next)
      await refreshItems()
      return true
    } catch (cause) {
      // The native layer reports a wrong password as one uniform message; the UI
      // adds nothing to it. A lockout is the exception: it has to say how long.
      reportError(cause)
      screen.value = 'locked'
      clearSensitiveState()
      // A failure changes the throttle, so the fresh counts have to be read back
      // before the lock screen can decide whether to offer the way out.
      await refreshStatusQuietly()
      return false
    } finally {
      busy.value = false
    }
  }

  /**
   * Mirrors the native lockout into a live countdown.
   *
   * The remaining time is derived from the absolute `lockedUntil` the native layer
   * reports, so the countdown stays correct across a reload and cannot drift as
   * timers are throttled in a background window.
   */
  function syncLockout(next?: VaultStatus): void {
    const until = (next ?? status.value)?.lockedUntil ?? undefined
    const remaining = until === undefined ? 0 : Math.max(0, Math.ceil((until - Date.now()) / 1_000))
    lockoutRemaining.value = remaining
    if (remaining > 0 && !lockoutTimer) {
      lockoutTimer = setInterval(() => {
        syncLockout()
        // `syncLockout` stops the timer once the lockout has run out.
      }, 1_000)
    } else if (remaining === 0 && lockoutTimer) {
      clearInterval(lockoutTimer)
      lockoutTimer = undefined
    }
  }

  async function refreshStatusQuietly(): Promise<void> {
    try {
      const next = await workbench.vault.status()
      status.value = next
      autoLockSeconds.value = next.autoLockSeconds
      syncLockout(next)
    } catch {
      // Best-effort: the lock screen simply keeps whatever it already knew.
    }
  }

  /**
   * Locks the vault and wipes local state.
   *
   * Locking is never confirmed and never fails the user's intent: even if the
   * native call fails, the local copy is dropped, because leaving decrypted data
   * on screen would be the worse outcome.
   */
  async function lock(): Promise<void> {
    try {
      await workbench.vault.lock()
    } catch (cause) {
      reportError(cause)
    } finally {
      clearSensitiveState()
      screen.value = 'locked'
      await refreshStatusAfterLock()
    }
  }

  /**
   * Re-reads vault status after a lock, without ever re-entering the unlocked
   * screen.
   *
   * Only an explicit `unlock`/`create` may move the UI back to unlocked. If a
   * status read here disagreed, showing an unlocked screen with no data — or
   * worse, triggering a reload — would be the wrong response; the next real
   * unlock reconciles everything.
   */
  async function refreshStatusAfterLock(): Promise<void> {
    try {
      const next = await workbench.vault.status()
      status.value = next
      autoLockSeconds.value = next.autoLockSeconds
      syncLockout(next)
      screen.value = next.exists ? 'locked' : 'uninitialized'
    } catch {
      // Status refresh is best-effort; the local state is already cleared.
    }
  }

  /** Applies a lock reported by the native supervisor (idle or resume-from-sleep). */
  async function handleNativeLock(): Promise<void> {
    clearSensitiveState()
    screen.value = 'locked'
    await refreshStatusAfterLock()
  }

  // ----------------------------------------------------------------------- items

  async function refreshItems(): Promise<void> {
    try {
      items.value = await workbench.vault.listItems()
      // Re-resolve the open item against the fresh list, so a deleted item does
      // not linger on screen.
      if (selectedId.value && !items.value.some((item) => item.id === selectedId.value)) {
        closeItem()
      }
    } catch (cause) {
      reportError(cause)
      if (AppError.from(cause).code === 'VAULT_LOCKED') await handleNativeLock()
    }
  }

  /** Opens one item. This is the only path that brings a password into the UI. */
  async function openItemById(id: string): Promise<void> {
    // Changing items always re-hides whatever was revealed before.
    hideSecrets()
    selectedId.value = id
    try {
      openItem.value = await workbench.vault.getItem(id)
    } catch (cause) {
      openItem.value = undefined
      reportError(cause)
      if (AppError.from(cause).code === 'VAULT_LOCKED') await handleNativeLock()
    }
  }

  function closeItem(): void {
    hideSecrets()
    selectedId.value = undefined
    openItem.value = undefined
  }

  async function toggleFavorite(id: string): Promise<void> {
    // Favourites live inside the encrypted payload, so toggling one requires
    // decrypting that item and re-encrypting it. Doing it natively means the
    // store never has to hold — or re-submit — the password to flip a boolean,
    // which matters now that a fetched item has been redacted.
    const current = items.value.find((entry) => entry.id === id)
    if (!current) return
    try {
      await workbench.vault.setFavorite(id, !current.favorite)
      await refreshItems()
    } catch (cause) {
      reportError(cause)
    }
  }

  async function saveDraft(): Promise<boolean> {
    busy.value = true
    error.value = undefined
    try {
      if (editingId.value) await workbench.vault.updateItem(editingId.value, draft.value)
      else await workbench.vault.createItem(draft.value)
      const savedId = editingId.value
      cancelEdit()
      await refreshItems()
      if (savedId) await openItemById(savedId)
      return true
    } catch (cause) {
      reportError(cause)
      return false
    } finally {
      busy.value = false
    }
  }

  async function deleteItem(id: string): Promise<boolean> {
    error.value = undefined
    try {
      const deleted = await workbench.vault.deleteItem(id)
      if (selectedId.value === id) closeItem()
      await refreshItems()
      return deleted
    } catch (cause) {
      reportError(cause)
      return false
    }
  }

  function startCreate(): void {
    hideSecrets()
    editingId.value = undefined
    draft.value = emptyVaultDraft()
    selectedId.value = undefined
    openItem.value = undefined
    generated.value = undefined
  }

  /**
   * Opens the editor for an existing item.
   *
   * The password has to be fetched first, because a fetched item is redacted.
   * Editing is an explicit user action, so this is one of the few moments a
   * secret is allowed into the UI — and it goes straight into the draft, never
   * into a rendered field.
   */
  async function startEdit(item: VaultItem): Promise<void> {
    hideSecrets()
    editingId.value = item.id
    draft.value = toPayload(item)
    generated.value = undefined
    if (!item.hasPassword) return
    try {
      const password = await workbench.vault.revealField(item.id)
      // Only fill the draft if the user is still editing this same item.
      if (editingId.value === item.id && password !== undefined) {
        draft.value = { ...draft.value, password }
      }
    } catch (cause) {
      reportError(cause)
    }
  }

  function cancelEdit(): void {
    editingId.value = undefined
    draft.value = emptyVaultDraft()
    generated.value = undefined
  }

  function toPayload(item: VaultItem): VaultItemPayload {
    return {
      title: item.title,
      username: item.username ?? '',
      // Deliberately empty: the item was redacted, so the password is unknown
      // here and is filled in by `startEdit` only when it is needed.
      password: '',
      url: item.url ?? '',
      notes: item.notes ?? '',
      tags: [...item.tags],
      customFields: item.customFields.map((field) => ({ ...field })),
      favorite: item.favorite,
    }
  }

  // ------------------------------------------------------------------- reveal

  /**
   * Reveals the open item's password for a bounded time.
   *
   * The value is fetched on demand and held only while it is displayed, then
   * dropped. Opening an item does not do this, so a detail pane that nobody
   * reveals never contains a password.
   */
  async function reveal(): Promise<void> {
    const id = selectedId.value
    if (!id) return
    try {
      const password = await workbench.vault.revealField(id)
      // A different item may have been opened while this was in flight.
      if (selectedId.value !== id || password === undefined) return
      revealedPassword.value = password
      revealed.value = true
      if (revealTimer) clearTimeout(revealTimer)
      revealTimer = setTimeout(() => {
        revealed.value = false
        revealedPassword.value = undefined
        revealTimer = undefined
      }, VAULT_REVEAL_SECONDS * 1_000)
    } catch (cause) {
      reportError(cause)
    }
  }

  function hideReveal(): void {
    revealed.value = false
    revealedPassword.value = undefined
    if (revealTimer) clearTimeout(revealTimer)
    revealTimer = undefined
  }

  /** Toggles one custom field's masked value, fetching it on first reveal. */
  async function toggleField(key: string, index?: number): Promise<void> {
    if (revealedFields.value[key] === true) {
      const next = { ...revealedFields.value }
      delete next[key]
      revealedFields.value = next
      const remaining = { ...revealedFieldValues.value }
      delete remaining[key]
      revealedFieldValues.value = remaining
      return
    }
    const id = selectedId.value
    if (id === undefined || index === undefined) return
    try {
      const value = await workbench.vault.revealField(id, index)
      // A different item may have been opened while this was in flight.
      if (selectedId.value !== id || value === undefined) return
      revealedFields.value = { ...revealedFields.value, [key]: true }
      revealedFieldValues.value = { ...revealedFieldValues.value, [key]: value }
    } catch (cause) {
      reportError(cause)
    }
  }

  function isFieldRevealed(key: string): boolean {
    return revealedFields.value[key] === true
  }

  /** The revealed text for a field, or `undefined` while it is still masked. */
  function revealedFieldValue(key: string): string | undefined {
    return revealedFieldValues.value[key]
  }

  // ---------------------------------------------------------------- clipboard

  /**
   * Copies a non-secret value, such as a username or a URL.
   *
   * The value is already in this process, so there is nothing to protect beyond
   * writing it. Passwords do **not** come through here — see `copyPasswordField`.
   */
  async function copy(value: string): Promise<void> {
    if (!value) return
    try {
      await workbench.vault.copySecret(value, 0)
      notice.value = 'vaultCopiedValue'
    } catch (cause) {
      reportError(cause)
    }
  }

  /**
   * Copies a secret field without ever holding its plaintext.
   *
   * The native layer decrypts the field, writes it to the clipboard, and
   * schedules the clear; the value is never returned, so a copied password does
   * not enter the WebView at all. `fieldIndex` of `undefined` selects the
   * password, a number selects a sensitive custom field.
   */
  async function copySecretField(fieldIndex?: number): Promise<void> {
    const id = selectedId.value
    if (!id) return
    const seconds = clipboardSeconds.value
    try {
      await workbench.vault.copyItemField(id, fieldIndex, seconds)
      notice.value = 'vaultCopiedSecret'
      startClipboardCountdown(seconds)
    } catch (cause) {
      // Clipboard clearing is platform-specific; when it is unavailable the user
      // is told plainly rather than left thinking the value will disappear.
      reportError(cause)
    }
  }

  function startClipboardCountdown(seconds: number): void {
    if (clipboardTimer) clearInterval(clipboardTimer)
    clipboardCountdown.value = seconds
    clipboardTimer = setInterval(() => {
      clipboardCountdown.value -= 1
      if (clipboardCountdown.value <= 0) {
        clipboardCountdown.value = 0
        if (clipboardTimer) clearInterval(clipboardTimer)
        clipboardTimer = undefined
      }
    }, 1_000)
  }

  // ----------------------------------------------------------------- generator

  async function regenerate(): Promise<void> {
    try {
      generated.value = await workbench.vault.generatePassword(generator.value)
    } catch (cause) {
      reportError(cause)
    }
  }

  function useGenerated(): void {
    if (!generated.value) return
    draft.value = { ...draft.value, password: generated.value }
    generated.value = undefined
  }

  function discardGenerated(): void {
    generated.value = undefined
  }

  // ----------------------------------------------------------------- settings

  /**
   * Persists the idle timeout. The value is a preference, not a secret, so it
   * lives in the ordinary settings table; the native session is told too, since
   * it enforces the timeout without the UI.
   */
  async function setAutoLock(seconds: number): Promise<void> {
    autoLockSeconds.value = seconds
    try {
      await workbench.vault.setAutoLock(seconds)
      await workbench.settings.save('vaultAutoLockSeconds', seconds)
    } catch (cause) {
      reportError(cause)
    }
  }

  async function setClipboardSeconds(seconds: number): Promise<void> {
    clipboardSeconds.value = seconds
    try {
      await workbench.settings.save('vaultClipboardClearSeconds', seconds)
    } catch (cause) {
      reportError(cause)
    }
  }

  /**
   * Reports user activity so the native auto-lock deadline moves.
   *
   * Throttled: this is called from pointer and keyboard handlers, and hitting
   * the native boundary on every event would be wasteful.
   */
  function touch(): void {
    const now = Date.now()
    if (now - lastTouch < 5_000) return
    lastTouch = now
    void workbench.vault.touch().catch(() => undefined)
  }

  // -------------------------------------------------------------------- backup

  async function exportBackup(path: string): Promise<boolean> {
    busy.value = true
    error.value = undefined
    try {
      await workbench.vault.exportBackup(path)
      notice.value = 'vaultBackupExported'
      return true
    } catch (cause) {
      reportError(cause)
      return false
    } finally {
      busy.value = false
    }
  }

  /**
   * Restores a vault from an encrypted backup.
   *
   * `replaceExisting` is required whenever a vault is already present — which is
   * the only situation a user with a vault is ever in, and the reason this is
   * reachable from the settings dialog and the lock screen rather than only from
   * the create screen. The native layer refuses an unflagged import, and writes a
   * safety copy of the current vault before replacing it.
   *
   * The restored vault is *locked*: a backup is ciphertext, and only the master
   * password that protected it can open it. That is why the screen moves to the
   * unlock prompt rather than into the vault.
   */
  async function importBackup(path: string, replaceExisting = false): Promise<boolean> {
    busy.value = true
    error.value = undefined
    try {
      const outcome = await workbench.vault.importBackup(path, replaceExisting)
      clearSensitiveState()
      screen.value = 'locked'
      await refreshStatusAfterLock()
      // Name the safety copy when there is one: it is the only way back if the
      // user picked the wrong file.
      if (outcome.safetyBackup) notice.value = 'vaultBackupRestoredWithSafety'
      else if (outcome.itemCount === 1) notice.value = 'vaultBackupRestoredOne'
      else notice.value = 'vaultBackupRestored'
      safetyBackupPath.value = outcome.safetyBackup
      return true
    } catch (cause) {
      reportError(cause)
      // A refused import writes nothing, so whatever was there is still usable.
      await refreshStatusAfterLock()
      return false
    } finally {
      busy.value = false
    }
  }

  /**
   * Removes the vault from this machine.
   *
   * The escape hatch for a vault that cannot be unlocked — a backup restored with
   * a password its owner no longer has, for instance. Everything decrypted is
   * dropped first, and the natively locked session is never relied upon alone.
   */
  async function destroyVault(): Promise<boolean> {
    busy.value = true
    error.value = undefined
    try {
      await workbench.vault.destroy()
      clearSensitiveState()
      screen.value = 'uninitialized'
      await refreshStatusAfterLock()
      notice.value = 'vaultDestroyed'
      return true
    } catch (cause) {
      reportError(cause)
      return false
    } finally {
      busy.value = false
    }
  }

  /**
   * Re-benchmarks the key-derivation cost for this machine.
   *
   * Nothing is re-encrypted: only the wrapper around the vault master key is
   * rewritten, so this stays quick however many items the vault holds. The native
   * layer never lowers an existing cost.
   */
  async function recalibrate(masterPassword: string): Promise<boolean> {
    busy.value = true
    error.value = undefined
    try {
      const next = await workbench.vault.recalibrate(masterPassword)
      status.value = next
      autoLockSeconds.value = next.autoLockSeconds
      syncLockout(next)
      notice.value = 'vaultRecalibrated'
      return true
    } catch (cause) {
      reportError(cause)
      await refreshStatusQuietly()
      return false
    } finally {
      busy.value = false
    }
  }

  async function changeMasterPassword(current: string, next: string): Promise<boolean> {    busy.value = true
    error.value = undefined
    try {
      await workbench.vault.changeMasterPassword(current, next)
      notice.value = 'vaultPasswordChanged'
      return true
    } catch (cause) {
      reportError(cause)
      return false
    } finally {
      busy.value = false
    }
  }

  async function openUrl(url: string): Promise<void> {
    try {
      await workbench.vault.openUrl(url)
    } catch (cause) {
      reportError(cause)
    }
  }

  function addCustomField(): void {
    draft.value = {
      ...draft.value,
      customFields: [...draft.value.customFields, { name: '', value: '', sensitive: true } as VaultItemField],
    }
  }

  function removeCustomField(index: number): void {
    draft.value = {
      ...draft.value,
      customFields: draft.value.customFields.filter((_, position) => position !== index),
    }
  }

  function addTag(tag: string): void {
    const trimmed = tag.trim()
    if (!trimmed || draft.value.tags.includes(trimmed)) return
    draft.value = { ...draft.value, tags: [...draft.value.tags, trimmed] }
  }

  function removeTag(tag: string): void {
    draft.value = { ...draft.value, tags: draft.value.tags.filter((value) => value !== tag) }
  }

  return {
    screen, items, query, selectedId, openItem, revealed, revealedPassword, revealedFields, revealedFieldValues, draft, editingId,
    busy, error, notice, safetyBackupPath, status, generated, generator, clipboardSeconds, clipboardCountdown,
    autoLockSeconds, lockoutRemaining, recoveryRevealed,
    visibleItems, favorites,
    strengthOf, initialize, refreshStatus, createVault, unlock, lock, handleNativeLock,
    refreshItems, openItemById, closeItem, toggleFavorite, saveDraft, deleteItem,
    startCreate, startEdit, cancelEdit,
    reveal, hideReveal, hideSecrets, toggleField, isFieldRevealed, revealedFieldValue, clearSensitiveState,
    copy, copySecretField, regenerate, useGenerated, discardGenerated,
    setAutoLock, setClipboardSeconds, touch, exportBackup, importBackup, destroyVault, changeMasterPassword, recalibrate, openUrl,
    addCustomField, removeCustomField, addTag, removeTag,
    reportError, dismissError, dismissNotice,
  }
})
