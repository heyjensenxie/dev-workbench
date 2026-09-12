/**
 * Password Vault store tests.
 *
 * The property under test is the one that matters most in the UI process: when
 * the vault locks — by hand, by idle timeout, or because the machine resumed
 * from sleep — nothing decrypted is left in memory. A screen that still holds a
 * password after a lock is the same as an unlocked vault.
 */

import type { VaultItem, VaultItemSummary } from '@dev-workbench/shared'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const SECRET = 'STORE-SECRET-PASSWORD-MARKER'

const bridge = vi.hoisted(() => ({
  vault: {
    status: vi.fn(),
    create: vi.fn(),
    unlock: vi.fn(),
    lock: vi.fn(),
    touch: vi.fn(),
    setAutoLock: vi.fn(),
    listItems: vi.fn(),
    getItem: vi.fn(),
    createItem: vi.fn(),
    updateItem: vi.fn(),
    deleteItem: vi.fn(),
    destroy: vi.fn(),
    setFavorite: vi.fn(),
    revealField: vi.fn(),
    copyItemField: vi.fn(),
    changeMasterPassword: vi.fn(),
    recalibrate: vi.fn(),
    exportBackup: vi.fn(),
    importBackup: vi.fn(),
    generatePassword: vi.fn(),
    copySecret: vi.fn(),
    clearClipboard: vi.fn(),
    openUrl: vi.fn(),
  },
  settings: { save: vi.fn() },
}))

vi.mock('../services/workbench', () => ({ workbench: bridge }))

import { useVaultStore } from './vault'

const LOCKED_STATUS = { exists: true, unlocked: false, formatVersion: 1, itemCount: 1, autoLockSeconds: 300 }
const UNLOCKED_STATUS = { exists: true, unlocked: true, formatVersion: 1, itemCount: 1, autoLockSeconds: 300 }
const MISSING_STATUS = { exists: false, unlocked: false, formatVersion: 1, itemCount: 0, autoLockSeconds: 300 }

const SUMMARY: VaultItemSummary = {
  id: 'item-1',
  title: 'GitHub',
  username: 'jensen@example.com',
  url: 'https://github.com',
  tags: ['work'],
  favorite: false,
  hasPassword: true,
  hasNotes: true,
  customFieldCount: 0,
  createdAt: 1,
  updatedAt: 2,
}

/**
 * A fetched item, exactly as the native layer returns it: **no password and no
 * sensitive custom field value**. Modelling this faithfully is the point — the
 * store tests would otherwise be asserting against a shape that never occurs.
 */
const FULL_ITEM: VaultItem = {
  id: 'item-1',
  title: 'GitHub',
  username: 'jensen@example.com',
  url: 'https://github.com',
  notes: 'recovery codes elsewhere',
  tags: ['work'],
  customFields: [],
  favorite: false,
  hasPassword: true,
  createdAt: 1,
  updatedAt: 2,
}

beforeEach(() => {
  setActivePinia(createPinia())
  vi.clearAllMocks()
  bridge.vault.status.mockResolvedValue(LOCKED_STATUS)
  bridge.vault.unlock.mockResolvedValue(UNLOCKED_STATUS)
  bridge.vault.create.mockResolvedValue(UNLOCKED_STATUS)
  bridge.vault.lock.mockResolvedValue(true)
  bridge.vault.listItems.mockResolvedValue([SUMMARY])
  bridge.vault.getItem.mockResolvedValue(FULL_ITEM)
  bridge.vault.generatePassword.mockResolvedValue('generated-value')
  bridge.vault.copySecret.mockResolvedValue(undefined)
  bridge.vault.copyItemField.mockResolvedValue(undefined)
  bridge.vault.revealField.mockResolvedValue(SECRET)
  bridge.vault.setFavorite.mockResolvedValue(FULL_ITEM)
  bridge.vault.touch.mockResolvedValue(undefined)
  bridge.vault.setAutoLock.mockResolvedValue(undefined)
  bridge.vault.updateItem.mockResolvedValue(FULL_ITEM)
  bridge.vault.deleteItem.mockResolvedValue(true)
  bridge.vault.importBackup.mockResolvedValue({ itemCount: 1, formatVersion: 1 })
  bridge.vault.destroy.mockResolvedValue(undefined)
  bridge.vault.recalibrate.mockResolvedValue(UNLOCKED_STATUS)
  bridge.settings.save.mockResolvedValue(undefined)
})

afterEach(() => {
  vi.useRealTimers()
})

describe('vault screen state', () => {
  it('starts locked and never unlocks implicitly', async () => {
    const store = useVaultStore()
    await store.initialize()

    expect(store.screen).toBe('locked')
    expect(store.items).toEqual([])
    expect(bridge.vault.listItems).not.toHaveBeenCalled()
    expect(bridge.vault.unlock).not.toHaveBeenCalled()
  })

  it('reports a missing vault as uninitialized', async () => {
    bridge.vault.status.mockResolvedValue(MISSING_STATUS)
    const store = useVaultStore()
    await store.initialize()
    expect(store.screen).toBe('uninitialized')
  })

  it('loads the list only when the vault is already unlocked', async () => {
    bridge.vault.status.mockResolvedValue(UNLOCKED_STATUS)
    const store = useVaultStore()
    await store.initialize()

    expect(store.screen).toBe('unlocked')
    expect(bridge.vault.listItems).toHaveBeenCalledTimes(1)
    expect(store.items).toHaveLength(1)
  })

  it('unlocks on demand and loads the list', async () => {
    const store = useVaultStore()
    await store.initialize()
    const ok = await store.unlock('correct-horse-battery')

    expect(ok).toBe(true)
    expect(store.screen).toBe('unlocked')
    expect(bridge.vault.unlock).toHaveBeenCalledWith('correct-horse-battery')
  })

  it('stays locked and surfaces the native message on a wrong password', async () => {
    bridge.vault.unlock.mockRejectedValue({ code: 'VAULT_BAD_PASSWORD', message: 'Incorrect master password.' })
    const store = useVaultStore()
    await store.initialize()

    const ok = await store.unlock('wrong-password-attempt')
    expect(ok).toBe(false)
    expect(store.screen).toBe('locked')
    expect(store.error).toBe('Incorrect master password.')
    expect(store.items).toEqual([])
  })

  it('creates a vault and moves straight to the unlocked screen', async () => {
    bridge.vault.status.mockResolvedValue(MISSING_STATUS)
    const store = useVaultStore()
    await store.initialize()

    const ok = await store.createVault('correct-horse-battery')
    expect(ok).toBe(true)
    expect(store.screen).toBe('unlocked')
  })
})

describe('locking clears every decrypted value', () => {
  async function unlockedStore() {
    bridge.vault.status.mockResolvedValue(UNLOCKED_STATUS)
    const store = useVaultStore()
    await store.initialize()
    await store.openItemById('item-1')
    store.query = 'github'
    await store.startEdit(FULL_ITEM)
    // Reveal last: entering edit mode deliberately hides anything revealed.
    await store.reveal()
    return store
  }

  it('drops the item list, the open item, the reveal, the query, and the draft', async () => {
    const store = await unlockedStore()
    expect(store.revealedPassword).toBe(SECRET)
    expect(store.revealed).toBe(true)

    await store.lock()

    expect(store.screen).toBe('locked')
    expect(store.items).toEqual([])
    expect(store.openItem).toBeUndefined()
    expect(store.selectedId).toBeUndefined()
    expect(store.revealed).toBe(false)
    expect(store.revealedPassword).toBeUndefined()
    expect(store.revealedFieldValues).toEqual({})
    expect(store.draft.title).toBe('')
    expect(store.draft.password).toBe('')
    expect(store.editingId).toBeUndefined()
    expect(store.query).toBe('')
    // Nothing decrypted survives anywhere in the store's public state.
    expect(JSON.stringify(store.$state)).not.toContain(SECRET)
  })

  it('clears state even when the native lock call fails', async () => {
    bridge.vault.lock.mockRejectedValue(new Error('native failure'))
    const store = await unlockedStore()

    await store.lock()

    expect(store.screen).toBe('locked')
    expect(store.openItem).toBeUndefined()
    expect(JSON.stringify(store.$state)).not.toContain(SECRET)
  })

  it('clears state when the native supervisor reports an automatic lock', async () => {
    const store = await unlockedStore()

    // Fired by the backend on idle timeout or resume-from-sleep.
    await store.handleNativeLock()

    expect(store.screen).toBe('locked')
    expect(store.items).toEqual([])
    expect(store.openItem).toBeUndefined()
    expect(store.revealed).toBe(false)
    expect(JSON.stringify(store.$state)).not.toContain(SECRET)
  })

  it('clears state when a read is refused because the vault locked underneath it', async () => {
    const store = await unlockedStore()
    bridge.vault.listItems.mockRejectedValue({ code: 'VAULT_LOCKED', message: 'vault is locked' })

    await store.refreshItems()

    expect(store.screen).toBe('locked')
    expect(store.openItem).toBeUndefined()
    expect(JSON.stringify(store.$state)).not.toContain(SECRET)
  })
})

describe('revealing passwords', () => {
  it('re-hides a revealed password after the timeout', async () => {
    vi.useFakeTimers()
    bridge.vault.status.mockResolvedValue(UNLOCKED_STATUS)
    const store = useVaultStore()
    await store.initialize()
    await store.openItemById('item-1')

    await store.reveal()
    expect(store.revealed).toBe(true)
    expect(store.revealedPassword).toBe(SECRET)

    vi.advanceTimersByTime(21_000)
    expect(store.revealed).toBe(false)
    expect(store.revealedPassword).toBeUndefined()
  })

  it('re-hides the previous password when another item is opened', async () => {
    bridge.vault.status.mockResolvedValue(UNLOCKED_STATUS)
    const store = useVaultStore()
    await store.initialize()
    await store.openItemById('item-1')
    await store.reveal()
    expect(store.revealed).toBe(true)

    bridge.vault.getItem.mockResolvedValue({ ...FULL_ITEM, id: 'item-2', title: 'GitLab' })
    await store.openItemById('item-2')

    expect(store.revealed).toBe(false)
    expect(store.revealedPassword).toBeUndefined()
    expect(store.isFieldRevealed('read-0')).toBe(false)
  })

  it('hides everything when the screen is left', async () => {
    bridge.vault.status.mockResolvedValue(UNLOCKED_STATUS)
    const store = useVaultStore()
    await store.initialize()
    await store.openItemById('item-1')
    await store.reveal()

    store.hideSecrets()
    expect(store.revealed).toBe(false)
    expect(store.revealedPassword).toBeUndefined()
    expect(store.isFieldRevealed('read-0')).toBe(false)
  })

  it('never holds a password just because an item was opened', async () => {
    // The core least-privilege property: opening an item must not put its
    // password into the UI process.
    bridge.vault.status.mockResolvedValue(UNLOCKED_STATUS)
    const store = useVaultStore()
    await store.initialize()
    await store.openItemById('item-1')

    expect(store.openItem?.title).toBe('GitHub')
    expect(store.openItem?.hasPassword).toBe(true)
    expect(store.revealed).toBe(false)
    expect(store.revealedPassword).toBeUndefined()
    expect(JSON.stringify(store.$state)).not.toContain(SECRET)
    expect(bridge.vault.revealField).not.toHaveBeenCalled()
  })

  it('ignores a reveal that arrives after the user moved to another item', async () => {
    // A reveal is an async round trip; the answer must not be applied to whatever
    // is on screen by the time it comes back.
    bridge.vault.status.mockResolvedValue(UNLOCKED_STATUS)
    const store = useVaultStore()
    await store.initialize()
    await store.openItemById('item-1')

    let release: ((value: string) => void) | undefined
    bridge.vault.revealField.mockImplementation(
      () => new Promise<string>((resolve) => { release = resolve }),
    )
    const pending = store.reveal()

    bridge.vault.getItem.mockResolvedValue({ ...FULL_ITEM, id: 'item-2', title: 'GitLab' })
    await store.openItemById('item-2')

    release?.(SECRET)
    await pending

    expect(store.revealed).toBe(false)
    expect(store.revealedPassword).toBeUndefined()
  })
})

describe('clipboard handling', () => {
  it('copies a password natively and never receives its plaintext', async () => {
    vi.useFakeTimers()
    bridge.vault.status.mockResolvedValue(UNLOCKED_STATUS)
    const store = useVaultStore()
    await store.initialize()
    await store.openItemById('item-1')
    store.setClipboardSeconds(30)

    await store.copySecretField()

    // The store asked the native layer to copy that item's password...
    expect(bridge.vault.copyItemField).toHaveBeenCalledWith('item-1', undefined, 30)
    // ...and never held the value itself.
    expect(JSON.stringify(store.$state)).not.toContain(SECRET)
    expect(store.notice).toBe('vaultCopiedSecret')
    expect(store.clipboardCountdown).toBe(30)

    vi.advanceTimersByTime(31_000)
    expect(store.clipboardCountdown).toBe(0)
  })

  it('copies a sensitive custom field by index', async () => {
    bridge.vault.status.mockResolvedValue(UNLOCKED_STATUS)
    const store = useVaultStore()
    await store.initialize()
    await store.openItemById('item-1')

    await store.copySecretField(2)

    expect(bridge.vault.copyItemField).toHaveBeenCalledWith('item-1', 2, store.clipboardSeconds)
  })

  it('copies non-secret values without scheduling a clear', async () => {
    const store = useVaultStore()
    await store.copy('jensen@example.com')

    expect(bridge.vault.copySecret).toHaveBeenCalledWith('jensen@example.com', 0)
    expect(store.notice).toBe('vaultCopiedValue')
    expect(store.clipboardCountdown).toBe(0)
  })

  it('reports a clipboard failure rather than pretending it succeeded', async () => {
    bridge.vault.status.mockResolvedValue(UNLOCKED_STATUS)
    bridge.vault.copyItemField.mockRejectedValue({ code: 'VAULT_ERROR', message: 'clipboard is busy' })
    const store = useVaultStore()
    await store.initialize()
    await store.openItemById('item-1')

    await store.copySecretField()

    expect(store.error).toBe('clipboard is busy')
  })

  it('ignores an empty copy request', async () => {
    const store = useVaultStore()
    await store.copy('')
    expect(bridge.vault.copySecret).not.toHaveBeenCalled()
  })
})

describe('generated passwords', () => {
  it('keeps a generated password out of the draft until it is applied', async () => {
    const store = useVaultStore()
    store.startCreate()

    await store.regenerate()
    expect(store.generated).toBe('generated-value')
    expect(store.draft.password).toBe('')

    store.useGenerated()
    expect(store.draft.password).toBe('generated-value')
    expect(store.generated).toBeUndefined()
  })

  it('discards a generated password without touching the draft', async () => {
    const store = useVaultStore()
    store.startCreate()
    await store.regenerate()

    store.discardGenerated()
    expect(store.generated).toBeUndefined()
    expect(store.draft.password).toBe('')
  })
})

describe('restoring an encrypted backup', () => {
  it('restores into a locked vault and asks for that backup\u2019s password', async () => {
    bridge.vault.status.mockResolvedValue(MISSING_STATUS)
    bridge.vault.importBackup.mockResolvedValue({ itemCount: 3, formatVersion: 1 })
    const store = useVaultStore()
    await store.initialize()
    expect(store.screen).toBe('uninitialized')

    // After the import the vault exists, so the next status read must report it.
    bridge.vault.status.mockResolvedValue(LOCKED_STATUS)
    const ok = await store.importBackup('C:/backups/vault.vaultbackup')

    expect(ok).toBe(true)
    expect(bridge.vault.importBackup).toHaveBeenCalledWith('C:/backups/vault.vaultbackup', false)
    // The backup is ciphertext: restoring it cannot unlock anything.
    expect(store.screen).toBe('locked')
    expect(store.items).toEqual([])
    expect(store.openItem).toBeUndefined()
    expect(store.notice).toBe('vaultBackupRestored')
  })

  it('reports a singular notice for a single-item backup', async () => {
    bridge.vault.status.mockResolvedValue(MISSING_STATUS)
    bridge.vault.importBackup.mockResolvedValue({ itemCount: 1, formatVersion: 1 })
    const store = useVaultStore()
    await store.initialize()
    bridge.vault.status.mockResolvedValue(LOCKED_STATUS)

    await store.importBackup('C:/backups/one.vaultbackup')

    expect(store.notice).toBe('vaultBackupRestoredOne')
  })

  it('leaves the create screen usable when the import is refused', async () => {
    // A refused import writes nothing, so the user can simply pick another file.
    bridge.vault.status.mockResolvedValue(MISSING_STATUS)
    bridge.vault.importBackup.mockRejectedValue({ code: 'VAULT_ERROR', message: 'backup file is not a Dev Workbench vault' })
    const store = useVaultStore()
    await store.initialize()

    const ok = await store.importBackup('C:/backups/not-ours.json')

    expect(ok).toBe(false)
    expect(store.error).toBe('backup file is not a Dev Workbench vault')
    expect(store.screen).toBe('uninitialized')
    expect(store.items).toEqual([])
  })

  it('never carries decrypted data across a restore', async () => {
    bridge.vault.status.mockResolvedValue(UNLOCKED_STATUS)
    const store = useVaultStore()
    await store.initialize()
    await store.openItemById('item-1')
    await store.reveal()
    expect(store.revealedPassword).toBe(SECRET)

    bridge.vault.importBackup.mockResolvedValue({ itemCount: 2, formatVersion: 1 })
    bridge.vault.status.mockResolvedValue(LOCKED_STATUS)
    await store.importBackup('C:/backups/two.vaultbackup', true)

    expect(JSON.stringify(store.$state)).not.toContain(SECRET)
    expect(store.revealed).toBe(false)
    expect(store.revealedPassword).toBeUndefined()
  })
})

describe('restoring over an existing vault', () => {
  it('passes the replace flag through and names the safety copy', async () => {
    // The path a user with a vault actually takes: settings, not the create screen.
    bridge.vault.status.mockResolvedValue(UNLOCKED_STATUS)
    bridge.vault.importBackup.mockResolvedValue({
      itemCount: 4,
      formatVersion: 1,
      safetyBackup: 'C:/data/backups/vault-before-restore-1.vaultbackup',
    })
    const store = useVaultStore()
    await store.initialize()
    bridge.vault.status.mockResolvedValue(LOCKED_STATUS)

    const ok = await store.importBackup('C:/backups/old.vaultbackup', true)

    expect(ok).toBe(true)
    expect(bridge.vault.importBackup).toHaveBeenCalledWith('C:/backups/old.vaultbackup', true)
    // The previous vault is recoverable, and the user is told from where.
    expect(store.notice).toBe('vaultBackupRestoredWithSafety')
    expect(store.safetyBackupPath).toBe('C:/data/backups/vault-before-restore-1.vaultbackup')
    expect(store.screen).toBe('locked')
  })

  it('refuses an unflagged restore even when a vault is present', async () => {
    bridge.vault.status.mockResolvedValue(UNLOCKED_STATUS)
    bridge.vault.importBackup.mockRejectedValue({ code: 'CONFLICT', message: 'a vault already exists in this location' })
    const store = useVaultStore()
    await store.initialize()

    expect(await store.importBackup('C:/backups/old.vaultbackup', false)).toBe(false)
    expect(bridge.vault.importBackup).toHaveBeenCalledWith('C:/backups/old.vaultbackup', false)
    expect(store.error).toBe('a vault already exists in this location')
  })
})

describe('unlock throttling', () => {
  it('hides the recovery actions until the vault is locked out', async () => {
    // Showing "remove this vault" on a fresh lock screen would put it one careless
    // click away for anyone who walked past the machine.
    bridge.vault.status.mockResolvedValue({ ...LOCKED_STATUS, failedAttempts: 0, lockedUntil: null })
    const store = useVaultStore()
    await store.initialize()
    expect(store.recoveryRevealed).toBe(false)

    bridge.vault.status.mockResolvedValue({ ...LOCKED_STATUS, failedAttempts: 4, lockedUntil: null })
    await store.refreshStatus()
    expect(store.recoveryRevealed).toBe(false)

    bridge.vault.status.mockResolvedValue({ ...LOCKED_STATUS, failedAttempts: 5, lockedUntil: Date.now() + 300_000 })
    await store.refreshStatus()
    expect(store.recoveryRevealed).toBe(true)
  })

  it('counts the lockout down and keeps the underlying attempts after it lapses', async () => {
    vi.useFakeTimers()
    const lockedUntil = Date.now() + 300_000
    bridge.vault.status.mockResolvedValue({ ...LOCKED_STATUS, failedAttempts: 5, lockedUntil })
    const store = useVaultStore()
    await store.initialize()

    expect(store.lockoutRemaining).toBe(300)
    expect(store.recoveryRevealed).toBe(true)

    vi.advanceTimersByTime(120_000)
    expect(store.lockoutRemaining).toBe(180)

    vi.advanceTimersByTime(200_000)
    expect(store.lockoutRemaining).toBe(0)
    // The counter itself is not reset by the lockout lapsing, which is what makes
    // the next failure escalate.
    expect(store.status?.failedAttempts).toBe(5)
    expect(store.recoveryRevealed).toBe(true)
  })

  it('reads the fresh throttle back after a failed unlock', async () => {
    bridge.vault.status.mockResolvedValue(LOCKED_STATUS)
    const store = useVaultStore()
    await store.initialize()

    bridge.vault.unlock.mockRejectedValue({ code: 'VAULT_LOCKED_OUT', message: 'Too many failed attempts.' })
    bridge.vault.status.mockResolvedValue({ ...LOCKED_STATUS, failedAttempts: 5, lockedUntil: Date.now() + 300_000 })
    const ok = await store.unlock('wrong')

    expect(ok).toBe(false)
    // Without this the lock screen could not know that it should now offer a way
    // out, and would keep showing a plain "wrong password".
    expect(store.recoveryRevealed).toBe(true)
    expect(store.lockoutRemaining).toBeGreaterThan(0)
  })

  it('clears the throttle display after a successful unlock', async () => {
    bridge.vault.status.mockResolvedValue({ ...LOCKED_STATUS, failedAttempts: 5, lockedUntil: Date.now() + 300_000 })
    const store = useVaultStore()
    await store.initialize()
    expect(store.lockoutRemaining).toBeGreaterThan(0)

    bridge.vault.unlock.mockResolvedValue({ ...UNLOCKED_STATUS, failedAttempts: 0, lockedUntil: null })
    await store.unlock('correct-horse-battery')

    expect(store.screen).toBe('unlocked')
    expect(store.lockoutRemaining).toBe(0)
    expect(store.recoveryRevealed).toBe(false)
  })
})

describe('re-benchmarking encryption strength', () => {
  it('reports the strengthened parameters and keeps the vault unlocked', async () => {
    bridge.vault.status.mockResolvedValue(UNLOCKED_STATUS)
    bridge.vault.recalibrate.mockResolvedValue({
      ...UNLOCKED_STATUS,
      kdf: {
        algorithm: 'argon2id',
        version: 1,
        memoryKib: 256 * 1024,
        timeCost: 3,
        parallelism: 4,
        meetsCurrentFloor: true,
      },
    })
    const store = useVaultStore()
    await store.initialize()

    const ok = await store.recalibrate('correct-horse-battery')

    expect(ok).toBe(true)
    expect(bridge.vault.recalibrate).toHaveBeenCalledWith('correct-horse-battery')
    // Re-wrapping the key does not lock the vault or disturb what is open.
    expect(store.screen).toBe('unlocked')
    expect(store.status?.kdf?.memoryKib).toBe(256 * 1024)
    expect(store.notice).toBe('vaultRecalibrated')
  })

  it('surfaces a wrong master password without changing anything', async () => {
    bridge.vault.status.mockResolvedValue(UNLOCKED_STATUS)
    bridge.vault.recalibrate.mockRejectedValue({ code: 'VAULT_BAD_PASSWORD', message: 'Incorrect master password.' })
    const store = useVaultStore()
    await store.initialize()

    const ok = await store.recalibrate('wrong')

    expect(ok).toBe(false)
    expect(store.error).toBe('Incorrect master password.')
    expect(store.notice).toBeUndefined()
  })
})

describe('removing the local vault', () => {
  it('returns to the create screen and drops everything decrypted', async () => {
    bridge.vault.status.mockResolvedValue(UNLOCKED_STATUS)
    const store = useVaultStore()
    await store.initialize()
    await store.openItemById('item-1')
    await store.reveal()
    expect(store.revealedPassword).toBe(SECRET)

    // After the destroy the vault no longer exists.
    bridge.vault.status.mockResolvedValue(MISSING_STATUS)
    const ok = await store.destroyVault()

    expect(ok).toBe(true)
    expect(bridge.vault.destroy).toHaveBeenCalledTimes(1)
    expect(store.screen).toBe('uninitialized')
    expect(store.items).toEqual([])
    expect(store.openItem).toBeUndefined()
    expect(store.revealed).toBe(false)
    expect(store.revealedPassword).toBeUndefined()
    expect(store.notice).toBe('vaultDestroyed')
    expect(JSON.stringify(store.$state)).not.toContain(SECRET)
  })

  it('unblocks the restore path afterwards', async () => {
    // The whole point of the escape hatch: a vault that cannot be unlocked must
    // not prevent restoring a different backup.
    bridge.vault.status.mockResolvedValue(LOCKED_STATUS)
    bridge.vault.importBackup.mockRejectedValue({ code: 'VAULT_ERROR', message: 'a vault already exists in this location' })
    const store = useVaultStore()
    await store.initialize()

    expect(await store.importBackup('C:/backups/two.vaultbackup', false)).toBe(false)
    expect(store.error).toBe('a vault already exists in this location')

    bridge.vault.status.mockResolvedValue(MISSING_STATUS)
    await store.destroyVault()

    expect(store.screen).toBe('uninitialized')
    expect(store.error).toBeUndefined()
  })

  it('reports a failure without clearing state it did not remove', async () => {    bridge.vault.status.mockResolvedValue(UNLOCKED_STATUS)
    bridge.vault.destroy.mockRejectedValue({ code: 'VAULT_ERROR', message: 'vault storage is unavailable' })
    const store = useVaultStore()
    await store.initialize()

    const ok = await store.destroyVault()

    expect(ok).toBe(false)
    expect(store.error).toBe('vault storage is unavailable')
    expect(store.screen).toBe('unlocked')
  })
})

describe('preferences and URLs', () => {
  it('persists the auto-lock timeout and tells the native session', async () => {
    const store = useVaultStore()
    await store.setAutoLock(600)

    expect(bridge.vault.setAutoLock).toHaveBeenCalledWith(600)
    expect(bridge.settings.save).toHaveBeenCalledWith('vaultAutoLockSeconds', 600)
    expect(store.autoLockSeconds).toBe(600)
  })

  it('persists the clipboard clear delay', async () => {
    const store = useVaultStore()
    await store.setClipboardSeconds(15)

    expect(bridge.settings.save).toHaveBeenCalledWith('vaultClipboardClearSeconds', 15)
    expect(store.clipboardSeconds).toBe(15)
  })
  it('delegates URL opening to the native boundary, which re-validates it', async () => {
    const store = useVaultStore()
    await store.openUrl('https://github.com')

    expect(bridge.vault.openUrl).toHaveBeenCalledWith('https://github.com')
  })

  it('throttles activity reporting', async () => {
    const store = useVaultStore()
    store.touch()
    store.touch()
    store.touch()

    expect(bridge.vault.touch).toHaveBeenCalledTimes(1)
  })
})
