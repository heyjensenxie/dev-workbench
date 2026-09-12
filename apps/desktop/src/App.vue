<script setup lang="ts">
import { Command, Copy, Cpu, Database, FolderGit2, Languages, LayoutDashboard, Lock, Minus, Moon, Network, PanelLeftClose, PanelLeftOpen, Play, Search, Send, Settings, ShieldCheck, Square, Sun, Wrench, X } from 'lucide-vue-next'
import { getCurrentWindow, type Window } from '@tauri-apps/api/window'
import { onMounted, onUnmounted, ref } from 'vue'
import { RouterLink, RouterView } from 'vue-router'
import { VAULT_BACKGROUND_LOCK_SECONDS } from '@dev-workbench/shared'
import logoUrl from './assets/logo.png'
import CommandPalette from './components/CommandPalette.vue'
import { useI18n } from './i18n'
import { subscribeToVaultLock } from './services/nativeBridge'
import { useSettingsStore } from './stores/settings'
import { useVaultStore } from './stores/vault'
import { useWorkbenchStore } from './stores/workbench'

const store = useWorkbenchStore()
const settings = useSettingsStore()
const vault = useVaultStore()
const { languageLabel, locale, t } = useI18n()
const paletteOpen = ref(false)
const isMaximized = ref(false)
const sidebarCollapsed = ref(initialSidebarCollapsed())
const version = __APP_VERSION__
let appWindow: Window | undefined
let stopWindowResizeListener: (() => void) | undefined
let stopVaultLockListener: (() => void) | undefined
let backgroundLockTimer: ReturnType<typeof setTimeout> | undefined

function initialSidebarCollapsed(): boolean {
  try {
    if (typeof localStorage === 'undefined') return false
    const savedPreference = localStorage.getItem('sidebar-collapsed')
    if (savedPreference !== null) return savedPreference === 'true'
    return typeof window !== 'undefined' && window.matchMedia?.('(max-width: 880px)').matches === true
  } catch {
    return false
  }
}

function toggleSidebar(): void {
  sidebarCollapsed.value = !sidebarCollapsed.value
  try {
    localStorage.setItem('sidebar-collapsed', String(sidebarCollapsed.value))
  } catch {
    // Sidebar preference is optional; the current session still reflects the change.
  }
}

function hasTauriWindow(): boolean {
  return typeof window !== 'undefined' && Reflect.has(window, '__TAURI_INTERNALS__')
}

function getAppWindow(): Window | undefined {
  if (typeof window === 'undefined') return undefined
  try {
    appWindow ??= getCurrentWindow()
    return appWindow
  } catch {
    return undefined
  }
}

async function refreshWindowState(): Promise<void> {
  const currentWindow = getAppWindow()
  if (!currentWindow) return
  try {
    isMaximized.value = await currentWindow.isMaximized()
  } catch (error) {
    console.warn('Unable to read the native window state.', error)
  }
}

async function minimizeWindow(): Promise<void> {
  const currentWindow = getAppWindow()
  if (!currentWindow) return
  try {
    await currentWindow.minimize()
  } catch (error) {
    console.error('Unable to minimize the native window.', error)
  }
}

async function toggleMaximize(): Promise<void> {
  const currentWindow = getAppWindow()
  if (!currentWindow) return
  try {
    await currentWindow.toggleMaximize()
    await refreshWindowState()
  } catch (error) {
    console.error('Unable to toggle the native window maximize state.', error)
  }
}

async function closeWindow(): Promise<void> {
  const currentWindow = getAppWindow()
  if (!currentWindow) return
  try {
    await currentWindow.close()
  } catch (error) {
    console.error('Unable to close the native window.', error)
  }
}

function handleTitlebarDoubleClick(event: MouseEvent): void {
  const target = event.target as HTMLElement
  if (target.closest('button, a, input, select, textarea')) return
  void toggleMaximize()
}

function toggleLanguage(): void {
  void settings.setLocale(locale.value === 'zh-CN' ? 'en-US' : 'zh-CN')
}
function handleKeydown(event: KeyboardEvent): void {
  if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'k') { event.preventDefault(); paletteOpen.value = true }
  // Manual lock. Deliberately not gated on the vault screen being open: locking
  // must work from anywhere, and locking never needs confirming.
  if ((event.ctrlKey || event.metaKey) && event.shiftKey && event.key.toLowerCase() === 'l') {
    event.preventDefault()
    void lockVaultNow()
  }
}

/** Locks the vault and returns to the lock screen, if either applies. */
async function lockVaultNow(): Promise<void> {
  if (vault.screen === 'unlocked' || vault.screen === 'locked') await vault.lock()
}

/**
 * Locks the vault when the window has been in the background for long enough.
 *
 * The native supervisor already locks on idle time, but a window that is
 * minimised or hidden stops producing the activity that keeps that deadline
 * moving, so this makes the intent explicit rather than incidental.
 */
function handleVisibilityChange(): void {
  if (typeof document === 'undefined') return
  if (document.visibilityState === 'hidden') {
    if (backgroundLockTimer) clearTimeout(backgroundLockTimer)
    backgroundLockTimer = setTimeout(() => { void lockVaultNow() }, VAULT_BACKGROUND_LOCK_SECONDS * 1_000)
    return
  }
  if (backgroundLockTimer) clearTimeout(backgroundLockTimer)
  backgroundLockTimer = undefined
}

onMounted(() => {
  void bootstrap()
  window.addEventListener('keydown', handleKeydown)
  document.addEventListener('visibilitychange', handleVisibilityChange)
  void refreshWindowState()
  if (hasTauriWindow()) {
    const currentWindow = getAppWindow()
    if (currentWindow) {
      void currentWindow.onResized(() => { void refreshWindowState() })
        .then((unlisten) => { stopWindowResizeListener = unlisten })
        .catch((error) => { console.warn('Unable to subscribe to native window resize events.', error) })
    }
    // The native supervisor reports locks it performed itself (idle timeout,
    // resume from sleep); the UI must drop everything it is holding when that
    // arrives, not just mark itself locked.
    void subscribeToVaultLock(() => {
      void vault.handleNativeLock()
      // Re-hide any revealed value even if the screen was already locked.
      vault.hideSecrets()
    })
      .then((unlisten) => { stopVaultLockListener = unlisten })
      .catch((error) => { console.warn('Unable to subscribe to native vault lock events.', error) })
  }
})

onUnmounted(() => {
  window.removeEventListener('keydown', handleKeydown)
  document.removeEventListener('visibilitychange', handleVisibilityChange)
  if (backgroundLockTimer) clearTimeout(backgroundLockTimer)
  stopWindowResizeListener?.()
  stopVaultLockListener?.()
})

async function bootstrap(): Promise<void> {
  await settings.load()
  await store.initialize()
  // The vault always starts locked; this only reads its status so the sidebar can
  // show the lock state. Nothing is decrypted here.
  await vault.refreshStatus()
  const lastProject = store.projects[0]
  if (settings.openLastProject && lastProject) await store.activateProject(lastProject)
}
</script>

<template>
  <div class="app-shell" :class="{ 'sidebar-collapsed': sidebarCollapsed }">
    <header class="titlebar" data-tauri-drag-region @dblclick="handleTitlebarDoubleClick">
      <div class="brand" aria-label="Dev Workbench"><img class="brand-mark" :src="logoUrl" alt="" width="23" height="23" /><strong>Dev Workbench</strong><span class="alpha">ALPHA</span></div>
      <button class="command-trigger" :aria-label="t('searchCommandBar')" @click="paletteOpen = true">
        <Search :size="15" />
        <span>{{ t('searchCommandBar') }}</span>
        <kbd>Ctrl K</kbd>
      </button>
      <div class="title-actions">
        <button class="language-button" :aria-label="t('toggleLanguage')" @click="toggleLanguage"><Languages :size="14" />{{ languageLabel }}</button>
        <button class="icon-button" :aria-label="t('toggleTheme')" @click="settings.toggleTheme()"><Sun v-if="settings.resolvedTheme === 'dark'" :size="16" /><Moon v-else :size="16" /></button>
        <RouterLink class="icon-button" to="/settings" :title="t('settings')"><Settings :size="16" /></RouterLink>
        <div class="window-controls" role="group" :aria-label="t('windowControls')" @mousedown.stop>
          <button class="window-control" type="button" :aria-label="t('minimize')" :title="t('minimize')" @click.stop="minimizeWindow"><Minus :size="14" /></button>
          <button class="window-control" type="button" :aria-label="isMaximized ? t('restore') : t('maximize')" :title="isMaximized ? t('restore') : t('maximize')" @click.stop="toggleMaximize">
            <Copy v-if="isMaximized" :size="14" />
            <Square v-else :size="14" />
          </button>
          <button class="window-control close-control" type="button" :aria-label="t('close')" :title="t('close')" @click.stop="closeWindow"><X :size="15" /></button>
        </div>
      </div>
    </header>
    <aside class="sidebar">
      <div class="sidebar-header">
        <span v-if="!sidebarCollapsed" class="sidebar-section-label">{{ t('workspace') }}</span>
        <button class="sidebar-toggle" type="button" :aria-label="sidebarCollapsed ? t('expandSidebar') : t('collapseSidebar')" :aria-expanded="!sidebarCollapsed" :data-tooltip="sidebarCollapsed ? t('expandSidebar') : t('collapseSidebar')" @click="toggleSidebar">
          <PanelLeftOpen v-if="sidebarCollapsed" :size="16" />
          <PanelLeftClose v-else :size="16" />
        </button>
      </div>
      <nav class="sidebar-navigation" aria-label="Workbench navigation">
        <div class="sidebar-group">
          <RouterLink class="sidebar-nav-item" to="/" :aria-label="t('overview')" :data-tooltip="sidebarCollapsed ? t('overview') : undefined">
            <span class="sidebar-icon-slot"><LayoutDashboard :size="18" /></span>
            <span v-if="!sidebarCollapsed" class="sidebar-nav-label">{{ t('overview') }}</span>
          </RouterLink>
          <RouterLink class="sidebar-nav-item" to="/projects" :aria-label="t('projects')" :data-tooltip="sidebarCollapsed ? t('projects') : undefined">
            <span class="sidebar-icon-slot"><FolderGit2 :size="18" /></span>
            <span v-if="!sidebarCollapsed" class="sidebar-nav-label">{{ t('projects') }}</span>
          </RouterLink>
          <RouterLink class="sidebar-nav-item" to="/services" :aria-label="t('services')" :data-tooltip="sidebarCollapsed ? t('services') : undefined">
            <span class="sidebar-icon-slot"><Play :size="18" /></span>
            <span v-if="!sidebarCollapsed" class="sidebar-nav-label">{{ t('services') }}</span>
          </RouterLink>
        </div>
        <div class="sidebar-group">
          <span v-if="!sidebarCollapsed" class="sidebar-section-label">{{ t('development') }}</span>
          <RouterLink class="sidebar-nav-item" to="/processes" :aria-label="t('processes')" :data-tooltip="sidebarCollapsed ? t('processes') : undefined">
            <span class="sidebar-icon-slot"><Cpu :size="18" /></span>
            <span v-if="!sidebarCollapsed" class="sidebar-nav-label">{{ t('processes') }}</span>
          </RouterLink>
          <RouterLink class="sidebar-nav-item" to="/ports" :aria-label="t('ports')" :data-tooltip="sidebarCollapsed ? t('ports') : undefined">
            <span class="sidebar-icon-slot"><Network :size="18" /></span>
            <span v-if="!sidebarCollapsed" class="sidebar-nav-label">{{ t('ports') }}</span>
          </RouterLink>
          <RouterLink class="sidebar-nav-item" to="/database" :aria-label="t('database')" :data-tooltip="sidebarCollapsed ? t('database') : undefined">
            <span class="sidebar-icon-slot"><Database :size="18" /></span>
            <span v-if="!sidebarCollapsed" class="sidebar-nav-label">{{ t('database') }}</span>
          </RouterLink>
        </div>
        <div class="sidebar-group">
          <span v-if="!sidebarCollapsed" class="sidebar-section-label">{{ t('utilitiesGroup') }}</span>
          <RouterLink class="sidebar-nav-item" to="/utilities" :aria-label="t('utilities')" :data-tooltip="sidebarCollapsed ? t('utilities') : undefined">
            <span class="sidebar-icon-slot"><Wrench :size="18" /></span>
            <span v-if="!sidebarCollapsed" class="sidebar-nav-label">{{ t('utilities') }}</span>
          </RouterLink>
          <RouterLink class="sidebar-nav-item" to="/api" :aria-label="t('apiWorkbench')" :data-tooltip="sidebarCollapsed ? t('apiWorkbench') : undefined">
            <span class="sidebar-icon-slot"><Send :size="18" /></span>
            <span v-if="!sidebarCollapsed" class="sidebar-nav-label">{{ t('apiWorkbench') }}</span>
          </RouterLink>
        </div>
        <!--
          The vault gets its own group rather than sitting among the utilities.
          It is not another developer tool: it is a separate security domain, and
          the navigation should make that legible. The lock indicator reflects
          live state so a locked vault is obvious at a glance.
        -->
        <div class="sidebar-group sidebar-group-security">
          <span v-if="!sidebarCollapsed" class="sidebar-section-label">{{ t('vaultGroup') }}</span>
          <RouterLink class="sidebar-nav-item" to="/vault" :aria-label="t('vault')" :data-tooltip="sidebarCollapsed ? t('vault') : undefined">
            <span class="sidebar-icon-slot"><ShieldCheck :size="18" /></span>
            <span v-if="!sidebarCollapsed" class="sidebar-nav-label">{{ t('vault') }}</span>
            <span v-if="!sidebarCollapsed && vault.screen === 'unlocked'" class="vault-state-dot" :title="t('vaultUnlock')"></span>
            <Lock v-else-if="!sidebarCollapsed && vault.screen !== 'loading'" class="vault-state-lock" :size="11" />
          </RouterLink>
        </div>
      </nav>
      <div class="sidebar-bottom">
        <RouterLink v-if="store.activeProject" class="sidebar-project" to="/projects" :aria-label="`${t('currentProject')}: ${store.activeProject.name}`" :data-tooltip="sidebarCollapsed ? `${t('currentProject')}: ${store.activeProject.name}` : undefined">
          <span class="sidebar-project-dot"></span>
          <span v-if="!sidebarCollapsed" class="sidebar-project-copy"><small>{{ t('currentProject') }}</small><strong>{{ store.activeProject.name }}</strong></span>
        </RouterLink>
        <nav class="sidebar-footer-navigation"><RouterLink class="sidebar-nav-item" to="/settings" :aria-label="t('settings')" :data-tooltip="sidebarCollapsed ? t('settings') : undefined">
          <span class="sidebar-icon-slot"><Settings :size="18" /></span>
          <span v-if="!sidebarCollapsed" class="sidebar-nav-label">{{ t('settings') }}</span>
        </RouterLink></nav>
      </div>
    </aside>
    <main class="workspace"><RouterView /></main>
    <footer class="statusbar">
      <span><span class="status-dot"></span>{{ t('localRuntimeReady') }}</span>
      <div class="statusbar-meta">
        <span v-if="store.activeProject">{{ t('currentProject') }}: <strong>{{ store.activeProject.name }}</strong></span>
        <span>{{ version }}</span>
        <button @click="paletteOpen = true"><Command :size="13" /> Ctrl K</button>
      </div>
    </footer>
    <CommandPalette v-model:open="paletteOpen" />
  </div>
</template>
