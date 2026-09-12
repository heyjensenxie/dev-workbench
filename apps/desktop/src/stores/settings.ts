import type { LocaleName, ThemeName, WorkbenchSettings } from '@dev-workbench/shared'
import { AppError, DEFAULT_VAULT_AUTO_LOCK_SECONDS, DEFAULT_VAULT_CLIPBOARD_CLEAR_SECONDS, clampLogLimit } from '@dev-workbench/shared'
import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { locale as activeLocale, setLocale as applyLocale } from '../i18n'
import { workbench } from '../services/workbench'

const DEFAULT_LOG_LIMIT = 1000

function initialTheme(): ThemeName {
  try {
    const saved = typeof localStorage === 'undefined' ? null : localStorage.getItem('theme')
    return saved === 'dark' || saved === 'light' || saved === 'system' ? saved : 'system'
  } catch {
    return 'system'
  }
}

function prefersDark(): boolean {
  return typeof window !== 'undefined' && typeof window.matchMedia === 'function'
    ? window.matchMedia('(prefers-color-scheme: dark)').matches
    : true
}

function applyTheme(theme: ThemeName): void {
  try {
    if (typeof document !== 'undefined') {
      const dark = theme === 'system' ? prefersDark() : theme === 'dark'
      document.documentElement.classList.toggle('dark', dark)
    }
    if (typeof localStorage !== 'undefined') localStorage.setItem('theme', theme)
  } catch {
    // Document storage is optional; the theme still applies for this session.
  }
}

export const useSettingsStore = defineStore('settings', () => {
  const theme = ref<ThemeName>(initialTheme())
  const resolvedTheme = ref<'dark' | 'light'>(theme.value === 'system' ? (prefersDark() ? 'dark' : 'light') : theme.value)
  const openLastProject = ref(true)
  const logLimit = ref(DEFAULT_LOG_LIMIT)
  const confirmBeforeKill = ref(true)
  const vaultAutoLockSeconds = ref(DEFAULT_VAULT_AUTO_LOCK_SECONDS)
  const vaultClipboardClearSeconds = ref(DEFAULT_VAULT_CLIPBOARD_CLEAR_SECONDS)
  const loaded = ref(false)
  const error = ref<string>()

  const locale = computed(() => activeLocale.value as LocaleName)

  applyTheme(theme.value)

  if (typeof window !== 'undefined' && typeof window.matchMedia === 'function') {
    window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', (event) => {
      if (theme.value !== 'system') return
      resolvedTheme.value = event.matches ? 'dark' : 'light'
      applyTheme(theme.value)
    })
  }

  async function load(): Promise<void> {
    try {
      const stored = await workbench.settings.load()
      if (stored.theme) theme.value = stored.theme
      if (stored.locale) applyLocale(stored.locale)
      if (stored.openLastProject !== undefined) openLastProject.value = stored.openLastProject
      if (stored.logLimit !== undefined) logLimit.value = stored.logLimit
      if (stored.confirmBeforeKill !== undefined) confirmBeforeKill.value = stored.confirmBeforeKill
      if (stored.vaultAutoLockSeconds !== undefined) vaultAutoLockSeconds.value = stored.vaultAutoLockSeconds
      if (stored.vaultClipboardClearSeconds !== undefined) vaultClipboardClearSeconds.value = stored.vaultClipboardClearSeconds
      applyTheme(theme.value)
      resolvedTheme.value = theme.value === 'system' ? (prefersDark() ? 'dark' : 'light') : theme.value
      loaded.value = true
    } catch (cause) {
      error.value = AppError.from(cause).message
    }
  }

  async function persist(key: keyof WorkbenchSettings, value: unknown): Promise<void> {
    error.value = undefined
    try {
      await workbench.settings.save(key, value)
    } catch (cause) {
      error.value = AppError.from(cause).message
    }
  }

  async function setTheme(next: ThemeName): Promise<void> {
    theme.value = next
    resolvedTheme.value = next === 'system' ? (prefersDark() ? 'dark' : 'light') : next
    applyTheme(next)
    await persist('theme', next)
  }

  async function toggleTheme(): Promise<void> {
    await setTheme(theme.value === 'dark' ? 'light' : 'dark')
  }

  async function setLocale(next: LocaleName): Promise<void> {
    applyLocale(next)
    await persist('locale', next)
  }

  async function setLogLimit(next: number): Promise<void> {
    logLimit.value = clampLogLimit(next)
    await persist('logLimit', logLimit.value)
  }

  async function setConfirmBeforeKill(next: boolean): Promise<void> {
    confirmBeforeKill.value = next
    await persist('confirmBeforeKill', next)
  }

  async function setOpenLastProject(next: boolean): Promise<void> {
    openLastProject.value = next
    await persist('openLastProject', next)
  }

  return {
    theme, resolvedTheme, locale, openLastProject, logLimit, confirmBeforeKill,
    vaultAutoLockSeconds, vaultClipboardClearSeconds, loaded, error,
    load, setTheme, toggleTheme, setLocale, setOpenLastProject, setLogLimit, setConfirmBeforeKill,
  }
})
