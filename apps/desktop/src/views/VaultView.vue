<script setup lang="ts">
/**
 * Password Vault screen.
 *
 * The screen has exactly three states — uninitialized, locked, unlocked — and it
 * never renders decrypted content outside the unlocked one. A few deliberate
 * choices are security behaviour rather than presentation:
 *
 * * The vault is never unlocked automatically. Opening this screen always shows
 *   the unlock prompt unless the vault is already unlocked in this process.
 * * Revealing a password is per item, time-bounded, and is undone when the item
 *   changes or the screen is left.
 * * Leaving the screen hides every revealed value, so returning to it cannot
 *   show something that was revealed earlier.
 * * Copying goes through the native clipboard command, which schedules its own
 *   clear; the UI only reports the countdown and states the platform limitation.
 */
import {
  Check,
  ClipboardCopy,
  Copy,
  Eye,
  EyeOff,
  ExternalLink,
  Gauge,
  KeyRound,
  Loader2,
  Lock,
  Plus,
  Search,
  Settings2,
  ShieldAlert,
  ShieldCheck,
  Star,
  Trash2,
  Upload,
  X,
} from 'lucide-vue-next'
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
import {
  MASTER_PASSWORD_MIN_LENGTH,
  VAULT_AUTO_LOCK_OPTIONS,
  VAULT_CLIPBOARD_CLEAR_OPTIONS,
  VAULT_DESTROY_CONFIRMATION,
  VAULT_GENERATOR_MAX_LENGTH,
  VAULT_GENERATOR_MIN_LENGTH,
  VAULT_KDF_FLOOR_MIB,
  VAULT_LOCKOUT_THRESHOLD,
  isVaultUrlOpenable,
  scoreMasterPassword,
  vaultUrlIssue,
} from '@dev-workbench/shared'
import type { VaultItemSummary } from '@dev-workbench/shared'
import { open, save } from '@tauri-apps/plugin-dialog'
import { useI18n, type MessageKey } from '../i18n'
import { useSettingsStore } from '../stores/settings'
import { useVaultStore } from '../stores/vault'

const { t } = useI18n()
const store = useVaultStore()
const settings = useSettingsStore()

// Unlock / create form state. These are the only places a master password is
// held, and they are cleared as soon as the operation finishes.
const masterPassword = ref('')
const confirmPassword = ref('')
const acknowledged = ref(false)
const showMasterPassword = ref(false)
const formError = ref<'tooShort' | 'mismatch' | undefined>()

// Whether the create/edit form is open. Kept separate from the store's
// `editingId` so "new item" can be cancelled without touching saved state.
const showEditor = ref(false)
const showGenerator = ref(false)
const showSettings = ref(false)
/** The dialog element, focused when it opens so Escape and Tab work from inside. */
const settingsDialog = ref<HTMLElement>()
/** Whether the restore confirmation is showing. */
const confirmRestore = ref(false)

// Destroy-the-vault dialog. The typed phrase is the only guard, deliberately:
// see `VAULT_DESTROY_CONFIRMATION` for why a password prompt would be theatre.
const showDestroy = ref(false)
const destroyDialog = ref<HTMLElement>()
const destroyConfirmation = ref('')
const destroyConfirmed = computed(() => destroyConfirmation.value.trim() === VAULT_DESTROY_CONFIRMATION)
const pendingDeleteId = ref<string>()
const currentMasterPassword = ref('')
const newMasterPassword = ref('')
/** Recalibration form: shown only while the user is reinforcing the vault. */
const showRecalibrate = ref(false)
const recalibratePassword = ref('')

const strength = computed(() => scoreMasterPassword(masterPassword.value))
const selected = computed(() => store.openItem)

/** Whether the vault is currently refusing attempts. */
const unlockThrottled = computed(() => store.lockoutRemaining > 0)

/** Attempts left before the vault starts locking. */
const attemptsLeft = computed(() =>
  Math.max(0, VAULT_LOCKOUT_THRESHOLD - (store.status?.failedAttempts ?? 0)))

/** `m:ss` remaining on the lockout, for a message that has to say how long. */
const lockoutClock = computed(() => {
  const total = store.lockoutRemaining
  const minutes = Math.floor(total / 60)
  const seconds = total % 60
  return `${minutes}:${String(seconds).padStart(2, '0')}`
})

/**
 * The notice banner text.
 *
 * Most notices are a fixed phrase; the restore notice names the safety copy the
 * native layer wrote, because that path is the user's only way back if they picked
 * the wrong file.
 */
const noticeText = computed(() => {
  const key = store.notice as MessageKey
  if (key === 'vaultBackupRestoredWithSafety') {
    return t(key, { path: store.safetyBackupPath ?? '' })
  }
  return t(key)
})

/** Reported Argon2id strength, so the UI states facts rather than implying them. */
const kdfMemoryMib = computed(() => Math.round((store.status?.kdf?.memoryKib ?? 0) / 1024))
const kdfBelowFloor = computed(() => store.status?.kdf?.meetsCurrentFloor === false)
/** The floor the native layer enforces, so both sides quote the same number. */
const kdfFloorMib = VAULT_KDF_FLOOR_MIB

/**
 * What to say about page-file protection.
 *
 * Only claimed while the vault is actually unlocked and the native layer reports
 * the lock in force; otherwise the honest statement is that it is not.
 */
const memoryLockKey = computed<MessageKey>(() => {
  if (store.screen !== 'unlocked') return 'vaultKdfMemoryNotLocked'
  return store.status?.memoryLocked ? 'vaultKdfMemoryLocked' : 'vaultKdfMemoryNotLocked'
})

const urlIssue = computed(() => {
  const url = store.draft.url ?? ''
  return url.trim() && vaultUrlIssue(url) ? vaultUrlIssue(url) : undefined
})

const strengthLabelKeys: Record<string, MessageKey> = {
  veryWeak: 'vaultStrengthVeryWeak',
  weak: 'vaultStrengthWeak',
  fair: 'vaultStrengthFair',
  strong: 'vaultStrengthStrong',
  excellent: 'vaultStrengthExcellent',
}

function strengthLabel(): string {
  return t(strengthLabelKeys[strength.value.label] ?? 'vaultStrengthVeryWeak')
}

function generatorStrengthLabel(): string {
  const label = scoreMasterPassword(store.generated ?? '').label
  return t(strengthLabelKeys[label] ?? 'vaultStrengthVeryWeak')
}

function autoLockLabel(seconds: number): string {
  return seconds < 60
    ? t('vaultSeconds', { seconds })
    : t('vaultMinutes', { minutes: Math.round(seconds / 60) })
}

function clipboardLabel(seconds: number): string {
  return seconds === 0 ? t('vaultOff') : t('vaultSeconds', { seconds })
}

function formatUpdated(timestamp: number): string {
  return new Date(timestamp).toLocaleString()
}

/** Stable key for a collapsible field, so index-based reveals cannot collide. */
function fieldKey(prefix: string, index: number): string {
  return prefix + '-' + String(index)
}

// --------------------------------------------------------------- key handling

async function submitUnlock(): Promise<void> {
  formError.value = undefined
  await store.unlock(masterPassword.value)
  // The password is dropped either way; nothing is kept around for a retry.
  masterPassword.value = ''
}

async function submitCreate(): Promise<void> {
  formError.value = undefined
  if (masterPassword.value.length < MASTER_PASSWORD_MIN_LENGTH) {
    formError.value = 'tooShort'
    return
  }
  if (masterPassword.value !== confirmPassword.value) {
    formError.value = 'mismatch'
    return
  }
  if (!acknowledged.value) return
  const ok = await store.createVault(masterPassword.value)
  if (!ok) return
  masterPassword.value = ''
  confirmPassword.value = ''
}

async function submitPasswordChange(): Promise<void> {
  formError.value = undefined
  if (newMasterPassword.value.length < MASTER_PASSWORD_MIN_LENGTH) {
    formError.value = 'tooShort'
    return
  }
  const ok = await store.changeMasterPassword(currentMasterPassword.value, newMasterPassword.value)
  if (!ok) return
  currentMasterPassword.value = ''
  newMasterPassword.value = ''
}

/** Closes the recalibration form again, wiping the typed password. */
function cancelRecalibrate(): void {
  showRecalibrate.value = false
  recalibratePassword.value = ''
}

async function submitRecalibrate(): Promise<void> {
  const ok = await store.recalibrate(recalibratePassword.value)
  recalibratePassword.value = ''
  if (ok) showRecalibrate.value = false
}

// ------------------------------------------------------------------- actions

async function copyUsername(): Promise<void> {
  if (selected.value?.username) await store.copy(selected.value.username)
}

/** Copies the password natively; the plaintext never passes through here. */
async function copyPassword(): Promise<void> {
  if (!selected.value?.hasPassword) return
  await store.copySecretField()
}

/** Copies a sensitive custom field the same way, by index. */
async function copyFieldValue(index: number, sensitive: boolean): Promise<void> {
  if (sensitive) await store.copySecretField(index)
}

async function toggleReveal(): Promise<void> {
  if (store.revealed) store.hideReveal()
  else await store.reveal()
}

async function followUrl(): Promise<void> {
  const url = selected.value?.url
  if (!url) return
  if (!isVaultUrlOpenable(url)) {
    const scheme = url.split(':')[0] ?? ''
    store.reportError(
      vaultUrlIssue(url) === 'unsupportedScheme'
        ? t('vaultUrlRejectedScheme', { scheme })
        : t('vaultUrlRejected'),
    )
    return
  }
  await store.openUrl(url)
}

function beginCreate(): void {
  store.startCreate()
  showEditor.value = true
  showGenerator.value = false
}

async function beginEdit(): Promise<void> {
  if (!selected.value) return
  // Fetching the password is part of opening the editor: the item itself was
  // redacted, so an edit would otherwise save an empty password.
  await store.startEdit(selected.value)
  showEditor.value = true
  showGenerator.value = false
}

function cancelEdit(): void {
  store.cancelEdit()
  showEditor.value = false
  showGenerator.value = false
}

async function submitDraft(): Promise<void> {
  const ok = await store.saveDraft()
  if (!ok) return
  showEditor.value = false
  showGenerator.value = false
}

async function confirmDelete(id: string): Promise<void> {
  await store.deleteItem(id)
  pendingDeleteId.value = undefined
}

async function toggleFavorite(id: string): Promise<void> {
  await store.toggleFavorite(id)
}

function openItem(item: VaultItemSummary): void {
  showEditor.value = false
  showGenerator.value = false
  void store.openItemById(item.id)
}

function useGeneratedPassword(): void {
  store.useGenerated()
  showGenerator.value = false
}

async function exportBackup(): Promise<void> {
  try {
    const path = await save({
      title: t('vaultBackupDialogTitle'),
      defaultPath: 'dev-workbench-vault.vaultbackup',
      filters: [{ name: 'Encrypted vault backup', extensions: ['vaultbackup'] }],
    })
    if (typeof path === 'string') await store.exportBackup(path)
  } catch (cause) {
    store.reportError(cause)
  }
}

/**
 * Picks a backup file and restores it.
 *
 * `replaceExisting` is true wherever a vault may already be present — the settings
 * dialog and the lock screen, which is every situation a user with a vault is in.
 * The native layer writes a safety copy of the current vault before replacing it
 * and returns its path, so choosing the wrong file is recoverable.
 *
 * The create screen passes `false`, keeping the strictest behaviour there: if a
 * vault appeared in the meantime, the import is refused rather than replacing it.
 */
async function restoreBackup(replaceExisting: boolean): Promise<void> {
  confirmRestore.value = false
  try {
    const path = await open({
      title: t('vaultRestoreDialogTitle'),
      multiple: false,
      directory: false,
      filters: [{ name: 'Encrypted vault backup', extensions: ['vaultbackup'] }],
    })
    if (typeof path !== 'string') return
    await store.importBackup(path, replaceExisting)
  } catch (cause) {
    store.reportError(cause)
  }
}

function submitTag(event: KeyboardEvent): void {
  const input = event.target as HTMLInputElement
  store.addTag(input.value)
  input.value = ''
}

function onAutoLockChange(event: Event): void {
  void store.setAutoLock(Number((event.target as HTMLSelectElement).value))
}

function onClipboardChange(event: Event): void {
  void store.setClipboardSeconds(Number((event.target as HTMLSelectElement).value))
}

/** Any real interaction pushes the native idle deadline out. */
function onActivity(): void {
  store.touch()
}

// ---------------------------------------------------------------- settings dialog

/**
 * Opens the security settings dialog.
 *
 * It is a modal rather than an inline panel because everything inside it is a
 * vault-wide policy decision — idle timeout, clipboard lifetime, the master
 * password itself — and none of it should be reachable while reading a credential.
 */
function openSettings(): void {
  showSettings.value = true
}

/**
 * Closes the dialog and wipes anything typed into it.
 *
 * The master password fields live only in this component, so closing has to clear
 * them explicitly; leaving a typed master password in component state after the
 * dialog is gone would defeat the point of keeping it out of storage.
 */
function closeSettings(): void {
  showSettings.value = false
  currentMasterPassword.value = ''
  newMasterPassword.value = ''
  cancelRecalibrate()
  formError.value = undefined
}

function onSettingsKeydown(event: KeyboardEvent): void {
  if (event.key === 'Escape') {
    event.stopPropagation()
    closeSettings()
  }
}

/**
 * Binds Escape for as long as the dialog is open, and moves focus into it.
 *
 * A listener on the dialog element alone would not be enough: the click that
 * opened the dialog leaves focus on the trigger button, so the dialog would never
 * see the key. Focus is moved explicitly for that reason, and released again on
 * close so the rest of the screen behaves normally.
 */
watch(showSettings, async (open) => {
  if (typeof window === 'undefined') return
  if (!open) {
    window.removeEventListener('keydown', onSettingsKeydown)
    return
  }
  window.addEventListener('keydown', onSettingsKeydown)
  await nextTick()
  settingsDialog.value?.focus()
})

// --------------------------------------------------------------- destroy dialog

function openDestroy(): void {
  showSettings.value = false
  destroyConfirmation.value = ''
  showDestroy.value = true
}

function closeDestroy(): void {
  showDestroy.value = false
  destroyConfirmation.value = ''
}

async function confirmDestroy(): Promise<void> {
  if (!destroyConfirmed.value) return
  const destroyed = await store.destroyVault()
  if (destroyed) closeDestroy()
}

watch(showDestroy, async (open) => {
  if (typeof window === 'undefined') return
  if (!open) {
    window.removeEventListener('keydown', onDestroyKeydown)
    return
  }
  window.addEventListener('keydown', onDestroyKeydown)
  await nextTick()
  destroyDialog.value?.focus()
})

function onDestroyKeydown(event: KeyboardEvent): void {
  if (event.key === 'Escape') {
    event.stopPropagation()
    closeDestroy()
  }
}

// If the vault locks while a dialog is open — by idle timeout, by resuming from
// sleep, or from elsewhere — it must not survive with a half-typed master password
// inside.
watch(() => store.screen, (screen) => {
  if (screen !== 'unlocked') closeSettings()
})

onMounted(() => {
  void store.initialize()
  store.setClipboardSeconds(settings.vaultClipboardClearSeconds)
  void store.setAutoLock(settings.vaultAutoLockSeconds)
})

onUnmounted(() => {
  // Leaving the screen hides everything revealed, so coming back cannot show a
  // password that was on screen before.
  store.hideSecrets()
  if (typeof window !== 'undefined') {
    window.removeEventListener('keydown', onSettingsKeydown)
    window.removeEventListener('keydown', onDestroyKeydown)
  }
  closeSettings()
  closeDestroy()
})
</script>

<template>
  <section class="view vault-view" @pointerdown="onActivity" @keydown="onActivity">
    <header class="view-header">
      <div>
        <p class="eyebrow">{{ t('vaultGroup') }}<span class="eyebrow-separator">/</span>{{ t('vault') }}</p>
        <h1>{{ t('vault') }}</h1>
        <p>{{ t('vaultSubtitle') }}</p>
      </div>
      <div class="header-actions">
        <template v-if="store.screen === 'unlocked'">
          <span class="vault-lock-hint"><kbd>{{ t('vaultLockShortcut') }}</kbd></span>
          <button class="ghost-button" type="button" :aria-label="t('vaultSettings')" :title="t('vaultSettings')" @click="openSettings">
            <Settings2 :size="14" />{{ t('vaultSettings') }}
          </button>
          <button class="ghost-button" type="button" @click="store.lock()">
            <Lock :size="13" />{{ t('vaultLockNow') }}
          </button>
        </template>
      </div>
    </header>

    <p v-if="store.error" class="error-banner">
      {{ store.error }}
      <button class="banner-dismiss" type="button" :aria-label="t('close')" @click="store.dismissError()"><X :size="12" /></button>
    </p>
    <p v-if="store.notice" class="notice-banner">
      {{ noticeText }}
      <template v-if="store.clipboardCountdown > 0"> {{ t('vaultClipboardClearNotice', { seconds: store.clipboardCountdown }) }}</template>
      <button class="banner-dismiss" type="button" :aria-label="t('close')" @click="store.dismissNotice()"><X :size="12" /></button>
    </p>

    <!-- Locked / uninitialized: no vault content is rendered at all. -->
    <div v-if="store.screen === 'loading'" class="vault-gate">
      <Loader2 class="spin" :size="22" />
      <p>{{ t('vaultLoading') }}</p>
    </div>

    <div v-else-if="store.screen === 'uninitialized'" class="vault-gate">
      <div class="vault-gate-card">
        <span class="vault-gate-icon"><ShieldCheck :size="22" /></span>
        <h2>{{ t('vaultCreateTitle') }}</h2>
        <p>{{ t('vaultCreateSubtitle') }}</p>

        <div class="vault-warning">
          <ShieldAlert :size="15" />
          <div>
            <strong>{{ t('vaultNoRecoveryTitle') }}</strong>
            <p>{{ t('vaultNoRecoveryDetail') }}</p>
          </div>
        </div>

        <form class="vault-form" @submit.prevent="submitCreate">
          <label class="field">
            <span>{{ t('vaultMasterPassword') }}</span>
            <div class="secret-input">
              <input v-model="masterPassword" :type="showMasterPassword ? 'text' : 'password'" autocomplete="new-password" />
              <button type="button" class="secret-toggle" :aria-label="showMasterPassword ? t('vaultHideSecret') : t('vaultShowSecret')" @click="showMasterPassword = !showMasterPassword">
                <EyeOff v-if="showMasterPassword" :size="14" /><Eye v-else :size="14" />
              </button>
            </div>
          </label>

          <div v-if="masterPassword" class="vault-strength" :data-level="strength.score">
            <span class="vault-strength-bar"><i :style="{ width: String(strength.score * 25) + '%' }"></i></span>
            <small>{{ t('vaultStrength') }}: {{ strengthLabel() }} · {{ strength.length }}</small>
          </div>
          <p class="vault-hint">{{ t('vaultLengthAdvice') }}</p>

          <label class="field">
            <span>{{ t('vaultConfirmPassword') }}</span>
            <input v-model="confirmPassword" type="password" autocomplete="new-password" />
          </label>

          <p v-if="formError === 'tooShort'" class="vault-field-error">{{ t('vaultPasswordTooShort') }}</p>
          <p v-else-if="formError === 'mismatch'" class="vault-field-error">{{ t('vaultPasswordMismatch') }}</p>

          <label class="vault-acknowledge">
            <input v-model="acknowledged" type="checkbox" />
            <span>{{ t('vaultNoRecoveryAcknowledge') }}</span>
          </label>

          <button class="primary" type="submit" :disabled="store.busy || !acknowledged">
            <Loader2 v-if="store.busy" class="spin" :size="13" />{{ store.busy ? t('vaultCreating') : t('vaultCreateVault') }}
          </button>
        </form>

        <!--
          The create screen also offers restore, because a user who has a backup
          and no vault is exactly who lands here, and before restore was reachable
          from the settings dialog and the lock screen this was the only place it
          could be found. It passes `replaceExisting = false`: nothing should be
          replaced on the screen that exists precisely because nothing is here.
        -->
        <div class="vault-restore">
          <div v-if="!confirmRestore">
            <button class="ghost-button" type="button" @click="confirmRestore = true">
              <Upload :size="13" />{{ t('vaultRestoreBackup') }}
            </button>
            <p class="vault-hint">{{ t('vaultRestoreBackupDetail') }}</p>
          </div>

          <div v-else class="vault-restore-confirm">
            <p>
              <strong>{{ t('vaultRestoreConfirmTitle') }}</strong>
              {{ t('vaultRestoreConfirmDetail') }}
            </p>
            <div class="vault-restore-actions">
              <button class="ghost-button" type="button" :disabled="store.busy" @click="confirmRestore = false">{{ t('vaultCancel') }}</button>
              <button class="primary" type="button" :disabled="store.busy" @click="restoreBackup(false)">
                <Loader2 v-if="store.busy" class="spin" :size="13" />{{ t('vaultRestoreBackup') }}
              </button>
            </div>
          </div>
        </div>
      </div>
    </div>

    <div v-else-if="store.screen === 'locked'" class="vault-gate">
      <div class="vault-gate-card">
        <span class="vault-gate-icon"><Lock :size="22" /></span>
        <h2>{{ t('vaultLockedTitle') }}</h2>
        <p>{{ t('vaultLockedSubtitle') }}</p>

        <form class="vault-form" @submit.prevent="submitUnlock">
          <label class="field">
            <span>{{ t('vaultMasterPassword') }}</span>
            <div class="secret-input">
              <input v-model="masterPassword" :type="showMasterPassword ? 'text' : 'password'" autocomplete="current-password" :disabled="unlockThrottled" />
              <button type="button" class="secret-toggle" :aria-label="showMasterPassword ? t('vaultHideSecret') : t('vaultShowSecret')" @click="showMasterPassword = !showMasterPassword">
                <EyeOff v-if="showMasterPassword" :size="14" /><Eye v-else :size="14" />
              </button>
            </div>
          </label>

          <!--
            A lockout has to say how long it lasts: the user cannot act, and
            "wrong password" would be actively misleading.
          -->
          <p v-if="unlockThrottled" class="vault-lockout">
            <ShieldAlert :size="14" />
            <span>{{ t('vaultLockedOut', { time: lockoutClock }) }}</span>
          </p>
          <p v-else-if="attemptsLeft > 0 && attemptsLeft < 3" class="vault-hint">
            {{ t('vaultAttemptsLeft', { count: attemptsLeft }) }}
          </p>

          <button class="primary" type="submit" :disabled="store.busy || !masterPassword || unlockThrottled">
            <Loader2 v-if="store.busy" class="spin" :size="13" />{{ store.busy ? t('vaultUnlocking') : t('vaultUnlock') }}
          </button>
        </form>

        <!--
          The recovery actions appear only once the vault has actually locked
          someone out. Showing them on a fresh lock screen would put "remove this
          vault" one careless click away for anyone who walked past the machine,
          and would suggest the owner needs an escape before there is anything to
          escape from.
        -->
        <div v-if="store.recoveryRevealed" class="vault-gate-actions">
          <button class="link-button" type="button" @click="restoreBackup(true)">
            {{ t('vaultRestoreFromLocked') }}
          </button>
          <button class="link-button vault-escape" type="button" @click="openDestroy">
            {{ t('vaultDestroyFromLocked') }}
          </button>
        </div>
      </div>
    </div>

    <!-- Unlocked -->
    <div v-else class="vault-body">
      <aside class="vault-list">
        <label class="search-field vault-search">
          <Search :size="14" />
          <input v-model="store.query" type="search" :placeholder="t('vaultSearchItems')" />
        </label>

        <button class="ghost-button vault-new" type="button" @click="beginCreate">
          <Plus :size="13" />{{ t('vaultNewItem') }}
        </button>

        <div class="vault-list-scroll">
          <template v-if="store.favorites.length">
            <p class="vault-list-group"><Star :size="11" />{{ t('vaultFavorites') }}</p>
            <button
              v-for="item in store.favorites"
              :key="'fav-' + item.id"
              type="button"
              class="vault-list-item"
              :class="{ active: item.id === store.selectedId }"
              @click="openItem(item)"
            >
              <span class="vault-list-title">{{ item.title }}</span>
              <small v-if="item.username">{{ item.username }}</small>
            </button>
          </template>

          <p class="vault-list-group"><KeyRound :size="11" />{{ t('vaultAllItems') }}</p>
          <button
            v-for="item in store.visibleItems"
            :key="item.id"
            type="button"
            class="vault-list-item"
            :class="{ active: item.id === store.selectedId }"
            @click="openItem(item)"
          >
            <span class="vault-list-title">{{ item.title }}</span>
            <small v-if="item.username">{{ item.username }}</small>
          </button>

          <p v-if="!store.items.length" class="quiet">{{ t('vaultNoItems') }}</p>
          <p v-else-if="!store.visibleItems.length" class="quiet">{{ t('vaultNoMatches') }}</p>
        </div>

        <p class="vault-list-count">{{ t('vaultItemCount', { count: store.items.length }) }}</p>
      </aside>

      <section class="vault-detail">
        <!-- Editor -->
        <form v-if="showEditor" class="vault-editor" @submit.prevent="submitDraft">
          <div class="vault-detail-head">
            <h2>{{ store.editingId ? t('vaultEditItemTitle') : t('vaultCreateItemTitle') }}</h2>
            <div class="vault-detail-actions">
              <button class="ghost-button" type="button" @click="cancelEdit"><X :size="13" />{{ t('vaultCancel') }}</button>
              <button class="primary" type="submit" :disabled="store.busy"><Check :size="13" />{{ t('vaultSave') }}</button>
            </div>
          </div>

          <label class="field"><span>{{ t('vaultTitle') }}</span><input v-model="store.draft.title" type="text" required /></label>
          <label class="field"><span>{{ t('vaultUsername') }}</span><input v-model="store.draft.username" type="text" autocomplete="off" /></label>

          <label class="field">
            <span>{{ t('vaultPassword') }}</span>
            <div class="secret-input">
              <input v-model="store.draft.password" :type="store.isFieldRevealed('draft-password') ? 'text' : 'password'" autocomplete="new-password" />
              <button type="button" class="secret-toggle" :aria-label="store.isFieldRevealed('draft-password') ? t('vaultHide') : t('vaultReveal')" @click="store.toggleField('draft-password')">                <EyeOff v-if="store.isFieldRevealed('draft-password')" :size="14" /><Eye v-else :size="14" />
              </button>
              <button type="button" class="secret-toggle" :aria-label="t('vaultGenerator')" @click="showGenerator = !showGenerator"><Settings2 :size="14" /></button>
            </div>
          </label>

          <div v-if="showGenerator" class="vault-generator">
            <div class="vault-generator-head">
              <strong>{{ t('vaultGenerator') }}</strong>
              <button class="ghost-button" type="button" @click="store.regenerate()">{{ t('vaultGeneratorRegenerate') }}</button>
            </div>
            <label class="field vault-generator-length">
              <span>{{ t('vaultGeneratorLength') }} · {{ store.generator.length }}</span>
              <input v-model.number="store.generator.length" type="range" :min="VAULT_GENERATOR_MIN_LENGTH" :max="VAULT_GENERATOR_MAX_LENGTH" />
            </label>
            <div class="vault-generator-toggles">
              <label><input v-model="store.generator.uppercase" type="checkbox" />{{ t('vaultGeneratorUppercase') }}</label>
              <label><input v-model="store.generator.lowercase" type="checkbox" />{{ t('vaultGeneratorLowercase') }}</label>
              <label><input v-model="store.generator.numbers" type="checkbox" />{{ t('vaultGeneratorNumbers') }}</label>
              <label><input v-model="store.generator.symbols" type="checkbox" />{{ t('vaultGeneratorSymbols') }}</label>
              <label><input v-model="store.generator.excludeAmbiguous" type="checkbox" />{{ t('vaultGeneratorExcludeAmbiguous') }}</label>
            </div>
            <code v-if="store.generated" class="vault-generated">{{ store.generated }}</code>
            <p v-else class="quiet">{{ t('vaultGeneratorEmpty') }}</p>
            <div v-if="store.generated" class="vault-generator-actions">
              <small>{{ t('vaultStrength') }}: {{ generatorStrengthLabel() }}</small>
              <button class="primary" type="button" @click="useGeneratedPassword">{{ t('vaultGeneratorUse') }}</button>
              <button class="ghost-button" type="button" @click="store.discardGenerated()">{{ t('vaultGeneratorDiscard') }}</button>
            </div>
          </div>

          <label class="field">
            <span>{{ t('vaultUrl') }}</span>
            <input v-model="store.draft.url" type="text" placeholder="https://" />
          </label>
          <p v-if="urlIssue" class="vault-field-error">{{ t('vaultUrlRejected') }}</p>

          <label class="field"><span>{{ t('vaultNotes') }}</span><textarea v-model="store.draft.notes" rows="4"></textarea></label>

          <div class="field">
            <span>{{ t('vaultTags') }}</span>
            <div class="vault-tags">
              <span v-for="tag in store.draft.tags" :key="tag" class="vault-tag">
                {{ tag }}
                <button type="button" :aria-label="t('vaultDelete')" @click="store.removeTag(tag)"><X :size="10" /></button>
              </span>
            </div>
            <input type="text" :placeholder="t('vaultTagPlaceholder')" @keydown.enter.prevent="submitTag" />
          </div>

          <div class="field">
            <span>{{ t('vaultCustomFields') }}</span>
            <div v-for="(field, index) in store.draft.customFields" :key="index" class="vault-custom-field">
              <input v-model="field.name" type="text" :placeholder="t('vaultFieldName')" />
              <input v-model="field.value" :type="field.sensitive && !store.isFieldRevealed(fieldKey('field', index)) ? 'password' : 'text'" :placeholder="t('vaultFieldValue')" />
              <label class="vault-inline-checkbox"><input v-model="field.sensitive" type="checkbox" />{{ t('vaultFieldSensitive') }}</label>
              <button type="button" class="ghost-button" :aria-label="t('vaultRemoveField')" @click="store.removeCustomField(index)"><Trash2 :size="12" /></button>
            </div>
            <button type="button" class="link-button" @click="store.addCustomField()">{{ t('vaultAddField') }}</button>
          </div>

          <label class="vault-inline-checkbox"><input v-model="store.draft.favorite" type="checkbox" />{{ t('vaultFavorite') }}</label>
        </form>

        <!-- Read-only detail -->
        <template v-else-if="selected">
          <div class="vault-detail-head">
            <h2>{{ selected.title }}</h2>
            <div class="vault-detail-actions">
              <button class="ghost-button" type="button" :title="selected.favorite ? t('vaultUnfavorite') : t('vaultFavorite')" @click="toggleFavorite(selected.id)">
                <Star :size="13" :fill="selected.favorite ? 'currentColor' : 'none'" />
              </button>
              <button class="ghost-button" type="button" @click="beginEdit"><Settings2 :size="13" />{{ t('vaultEdit') }}</button>
              <button class="ghost-button danger-button" type="button" @click="pendingDeleteId = selected.id"><Trash2 :size="13" />{{ t('vaultDelete') }}</button>
            </div>
          </div>

          <div v-if="pendingDeleteId === selected.id" class="vault-confirm">
            <p>{{ t('vaultDeleteConfirm', { title: selected.title }) }}</p>
            <p class="quiet">{{ t('vaultDeleteNoErase') }}</p>
            <div class="vault-confirm-actions">
              <button class="ghost-button danger-button" type="button" @click="confirmDelete(selected.id)">{{ t('vaultDelete') }}</button>
              <button class="ghost-button" type="button" @click="pendingDeleteId = undefined">{{ t('vaultCancel') }}</button>
            </div>
          </div>

          <dl class="vault-fields">
            <div v-if="selected.username" class="vault-field">
              <dt>{{ t('vaultUsername') }}</dt>
              <dd>
                <code>{{ selected.username }}</code>
                <button class="ghost-button" type="button" :title="t('vaultCopyUsername')" @click="copyUsername"><ClipboardCopy :size="12" /></button>
              </dd>
            </div>

            <!--
              The password is not part of the fetched item. It appears here only
              while revealed, having been requested field-by-field.
            -->
            <div v-if="selected.hasPassword" class="vault-field">
              <dt>{{ t('vaultPassword') }}</dt>
              <dd>
                <code class="vault-secret">{{ store.revealed ? store.revealedPassword : '••••••••••••' }}</code>
                <button class="ghost-button" type="button" :title="store.revealed ? t('vaultHide') : t('vaultReveal')" @click="toggleReveal">
                  <EyeOff v-if="store.revealed" :size="12" /><Eye v-else :size="12" />
                </button>
                <button class="ghost-button" type="button" :title="t('vaultCopyPassword')" @click="copyPassword"><Copy :size="12" /></button>
              </dd>
            </div>

            <div v-if="selected.url" class="vault-field">
              <dt>{{ t('vaultUrl') }}</dt>
              <dd>
                <code>{{ selected.url }}</code>
                <button class="ghost-button" type="button" :disabled="!isVaultUrlOpenable(selected.url)" :title="t('vaultOpenUrl')" @click="followUrl"><ExternalLink :size="12" /></button>
              </dd>
            </div>

            <div v-if="selected.tags.length" class="vault-field">
              <dt>{{ t('vaultTagsLabel') }}</dt>
              <dd><span v-for="tag in selected.tags" :key="tag" class="vault-tag">{{ tag }}</span></dd>
            </div>

            <div v-if="selected.notes" class="vault-field vault-field-block">
              <dt>{{ t('vaultNotes') }}</dt>
              <dd>{{ selected.notes }}</dd>
            </div>

            <div v-if="selected.customFields.length" class="vault-field vault-field-block">
              <dt>{{ t('vaultCustomFields') }}</dt>
              <dd>
                <div v-for="(field, index) in selected.customFields" :key="fieldKey('read', index)" class="vault-custom-read">
                  <span>{{ field.name }}</span>
                  <!--
                    A sensitive field's value is not in the fetched item either.
                    It is shown only after an explicit reveal, which fetches that
                    one field by index.
                  -->
                  <code v-if="!field.sensitive">{{ field.value }}</code>
                  <code v-else-if="store.isFieldRevealed(fieldKey('read', index))">{{ store.revealedFieldValue(fieldKey('read', index)) }}</code>
                  <code v-else>••••••••</code>
                  <button v-if="field.sensitive" class="ghost-button" type="button" :aria-label="t('vaultReveal')" @click="store.toggleField(fieldKey('read', index), index)">
                    <EyeOff v-if="store.isFieldRevealed(fieldKey('read', index))" :size="12" /><Eye v-else :size="12" />
                  </button>
                  <button class="ghost-button" type="button" :aria-label="t('vaultCopyPassword')" @click="copyFieldValue(index, field.sensitive)"><Copy :size="12" /></button>
                </div>
              </dd>
            </div>

            <div class="vault-field vault-field-block">
              <dt>{{ t('vaultUpdated') }}</dt>
              <dd>{{ formatUpdated(selected.updatedAt) }}</dd>
            </div>
          </dl>

          <p class="vault-note">{{ t('vaultClipboardLimitation') }}</p>
          <p class="vault-note quiet">{{ t('vaultTotpPlanned') }}</p>
        </template>

        <div v-else class="vault-empty">
          <KeyRound :size="22" />
          <p>{{ t('vaultSelectItem') }}</p>
        </div>
      </section>
    </div>

    <!--
      Security settings are a modal, reached from the header. Everything in here is
      a vault-wide policy decision, so it is deliberately kept off the credential
      view rather than sitting beside a password. Closing the dialog — or the vault
      locking underneath it — clears any typed master password.
    -->
    <div v-if="showSettings" class="modal-backdrop" @mousedown.self="closeSettings">
      <section
        ref="settingsDialog"
        class="vault-settings-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="vault-settings-title"
        tabindex="-1"
      >
        <header>
          <div>
            <span class="section-kicker">{{ t('vaultGroup') }}</span>
            <h2 id="vault-settings-title">{{ t('vaultSettings') }}</h2>
          </div>
          <button class="icon-button" type="button" :aria-label="t('close')" @click="closeSettings"><X :size="16" /></button>
        </header>

        <div class="vault-settings-body">
          <div class="vault-settings-pair">
            <label class="field">
              <span>{{ t('vaultAutoLock') }}</span>
              <select :value="store.autoLockSeconds" @change="onAutoLockChange">
                <option v-for="option in VAULT_AUTO_LOCK_OPTIONS" :key="option" :value="option">{{ autoLockLabel(option) }}</option>
              </select>
            </label>

            <label class="field">
              <span>{{ t('vaultClipboardClear') }}</span>
              <select :value="store.clipboardSeconds" @change="onClipboardChange">
                <option v-for="option in VAULT_CLIPBOARD_CLEAR_OPTIONS" :key="option" :value="option">{{ clipboardLabel(option) }}</option>
              </select>
            </label>
          </div>

          <!--
            Encryption strength is reported rather than assumed. A vault created
            below the current floor says so, instead of being described as equally
            strong as a new one.
          -->
          <section class="vault-settings-section">
            <h3>{{ t('vaultKdfTitle') }}</h3>
            <p class="quiet">
              {{ t('vaultKdfDetail', { memory: kdfMemoryMib, time: store.status?.kdf?.timeCost ?? '—', parallelism: store.status?.kdf?.parallelism ?? '—' }) }}
            </p>
            <p v-if="kdfBelowFloor" class="vault-field-error">{{ t('vaultKdfBelowFloor', { memory: kdfFloorMib }) }}</p>
            <p class="quiet">{{ t(memoryLockKey) }}</p>

            <div v-if="!showRecalibrate">
              <button class="ghost-button" type="button" @click="showRecalibrate = true">
                <Gauge :size="13" />{{ t('vaultRecalibrate') }}
              </button>
            </div>
            <form v-else class="vault-form" @submit.prevent="submitRecalibrate">
              <p class="quiet">{{ t('vaultRecalibrateDetail') }}</p>
              <label class="field">
                <span>{{ t('vaultCurrentPassword') }}</span>
                <input v-model="recalibratePassword" type="password" autocomplete="current-password" />
              </label>
              <div>
                <button class="ghost-button" type="button" @click="cancelRecalibrate">{{ t('vaultCancel') }}</button>
                <button class="primary" type="submit" :disabled="store.busy || !recalibratePassword">
                  <Loader2 v-if="store.busy" class="spin" :size="13" />{{ t('vaultRecalibrateSubmit') }}
                </button>
              </div>
            </form>
          </section>

          <section class="vault-settings-section">
            <h3>{{ t('vaultChangeMasterPassword') }}</h3>
            <form class="vault-form" @submit.prevent="submitPasswordChange">
              <label class="field"><span>{{ t('vaultCurrentPassword') }}</span><input v-model="currentMasterPassword" type="password" autocomplete="current-password" /></label>
              <label class="field"><span>{{ t('vaultNewPassword') }}</span><input v-model="newMasterPassword" type="password" autocomplete="new-password" /></label>
              <p v-if="formError === 'tooShort'" class="vault-field-error">{{ t('vaultPasswordTooShort') }}</p>
              <div>
                <button class="primary" type="submit" :disabled="store.busy || !currentMasterPassword || !newMasterPassword">{{ t('vaultChangePasswordSubmit') }}</button>
              </div>
            </form>
          </section>

          <section class="vault-settings-section">
            <h3>{{ t('vaultBackupTitle') }}</h3>
            <p class="quiet">{{ t('vaultExportBackupDetail') }}</p>
            <div>
              <button class="ghost-button" type="button" @click="exportBackup"><ShieldCheck :size="13" />{{ t('vaultExportBackup') }}</button>
              <button class="ghost-button" type="button" @click="restoreBackup(true)"><Upload :size="13" />{{ t('vaultRestoreBackup') }}</button>
            </div>
            <p class="quiet">{{ t('vaultRestoreReplaces') }}</p>
          </section>

          <section class="vault-settings-section quiet-block">
            <h3>{{ t('vaultBoundaryTitle') }}</h3>
            <p class="quiet">{{ t('vaultBoundaryDetail') }}</p>
          </section>

          <section class="vault-settings-section danger-zone">
            <h3>{{ t('vaultDestroy') }}</h3>
            <p class="quiet">{{ t('vaultDestroyDetail') }}</p>
            <div>
              <button class="ghost-button danger-button" type="button" @click="openDestroy">
                <Trash2 :size="13" />{{ t('vaultDestroy') }}
              </button>
            </div>
          </section>
        </div>

        <footer>
          <button class="ghost-button" type="button" @click="closeSettings">{{ t('close') }}</button>
        </footer>
      </section>
    </div>

    <!--
      Removing the vault is the one irreversible action, so it is its own dialog
      behind a typed confirmation rather than a button that acts immediately. It is
      reachable both from the settings dialog and from the lock screen, because the
      case that needs it most is a vault that cannot be unlocked.
    -->
    <div v-if="showDestroy" class="modal-backdrop" @mousedown.self="closeDestroy">
      <section
        ref="destroyDialog"
        class="vault-settings-dialog vault-destroy-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="vault-destroy-title"
        tabindex="-1"
      >
        <header>
          <div>
            <span class="section-kicker danger-kicker">{{ t('vaultGroup') }}</span>
            <h2 id="vault-destroy-title">{{ t('vaultDestroyConfirmTitle') }}</h2>
          </div>
          <button class="icon-button" type="button" :aria-label="t('close')" @click="closeDestroy"><X :size="16" /></button>
        </header>

        <div class="vault-settings-body">
          <p>{{ t('vaultDestroyConfirmDetail') }}</p>
          <p class="quiet">{{ t('vaultDestroyNoErase') }}</p>
          <label class="field">
            <span>{{ t('vaultDestroyTypeToConfirm', { phrase: VAULT_DESTROY_CONFIRMATION }) }}</span>
            <input v-model="destroyConfirmation" type="text" autocomplete="off" spellcheck="false" :placeholder="VAULT_DESTROY_CONFIRMATION" />
          </label>
        </div>

        <footer>
          <button class="ghost-button" type="button" @click="closeDestroy">{{ t('vaultCancel') }}</button>
          <button
            class="ghost-button danger-button"
            type="button"
            :disabled="store.busy || !destroyConfirmed"
            @click="confirmDestroy"
          >
            <Loader2 v-if="store.busy" class="spin" :size="13" /><Trash2 v-else :size="13" />{{ t('vaultDestroySubmit') }}
          </button>
        </footer>
      </section>
    </div>
  </section>
</template>
