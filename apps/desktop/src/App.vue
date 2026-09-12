<script setup lang="ts">
import { Command, Copy, Cpu, FolderGit2, Languages, LayoutDashboard, Minus, Moon, Network, Play, Search, Settings, Square, Sun, Wrench, X } from 'lucide-vue-next'
import { getCurrentWindow, type Window } from '@tauri-apps/api/window'
import { onMounted, onUnmounted, ref } from 'vue'
import { RouterLink, RouterView } from 'vue-router'
import logoUrl from './assets/logo.png'
import CommandPalette from './components/CommandPalette.vue'
import { useI18n } from './i18n'
import { useSettingsStore } from './stores/settings'
import { useWorkbenchStore } from './stores/workbench'

const store = useWorkbenchStore()
const settings = useSettingsStore()
const { languageLabel, locale, t } = useI18n()
const paletteOpen = ref(false)
const isMaximized = ref(false)
const version = __APP_VERSION__
let appWindow: Window | undefined
let stopWindowResizeListener: (() => void) | undefined

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
}
onMounted(() => {
  void bootstrap()
  window.addEventListener('keydown', handleKeydown)
  void refreshWindowState()
  if (hasTauriWindow()) {
    const currentWindow = getAppWindow()
    if (currentWindow) {
      void currentWindow.onResized(() => { void refreshWindowState() })
        .then((unlisten) => { stopWindowResizeListener = unlisten })
        .catch((error) => { console.warn('Unable to subscribe to native window resize events.', error) })
    }
  }
})

onUnmounted(() => {
  window.removeEventListener('keydown', handleKeydown)
  stopWindowResizeListener?.()
})

async function bootstrap(): Promise<void> {
  await settings.load()
  await store.initialize()
  const lastProject = store.projects[0]
  if (settings.openLastProject && lastProject) await store.activateProject(lastProject)
}
</script>

<template>
  <div class="app-shell">
    <header class="titlebar" data-tauri-drag-region @dblclick="handleTitlebarDoubleClick">
      <div class="brand"><img class="brand-mark" :src="logoUrl" alt="" width="23" height="23" /><strong>Dev Workbench</strong><span class="alpha">ALPHA</span></div>
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
      <nav aria-label="Workbench navigation">
        <span class="sidebar-section-label">{{ t('workspace') }}</span>
        <RouterLink to="/"><LayoutDashboard :size="17" />{{ t('overview') }}</RouterLink>
        <RouterLink to="/projects"><FolderGit2 :size="17" />{{ t('projects') }}</RouterLink>
        <RouterLink to="/services"><Play :size="17" />{{ t('services') }}</RouterLink>
        <span class="sidebar-section-label sidebar-section-spacer">{{ t('development') }}</span>
        <RouterLink to="/processes"><Cpu :size="17" />{{ t('processes') }}</RouterLink>
        <RouterLink to="/ports"><Network :size="17" />{{ t('ports') }}</RouterLink>
        <span class="sidebar-section-label sidebar-section-spacer">{{ t('utilitiesGroup') }}</span>
        <RouterLink to="/utilities"><Wrench :size="17" />{{ t('utilities') }}</RouterLink>
      </nav>
      <div class="sidebar-bottom">
        <div v-if="store.activeProject" class="sidebar-project">
          <span class="sidebar-project-dot"></span>
          <span><small>{{ t('currentProject') }}</small><strong>{{ store.activeProject.name }}</strong></span>
        </div>
        <nav><RouterLink to="/settings"><Settings :size="17" />{{ t('settings') }}</RouterLink></nav>
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
