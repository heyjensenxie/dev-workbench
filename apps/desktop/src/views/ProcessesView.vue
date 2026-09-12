<script setup lang="ts">
import { formatBytes, formatUptime } from '@dev-workbench/shared'
import { ListTree, RefreshCw, Search, Skull } from 'lucide-vue-next'
import { computed, onMounted } from 'vue'
import { useI18n } from '../i18n'
import { useSettingsStore } from '../stores/settings'
import { useSystemStore } from '../stores/system'

const { t } = useI18n()
const system = useSystemStore()
const settings = useSettingsStore()

const names = computed(() => {
  const byPid = new Map(system.processes.map((process) => [process.pid, process.name]))
  return byPid
})

function parentLabel(parentPid: number | undefined): string {
  if (parentPid === undefined) return '—'
  return `${names.value.get(parentPid) ?? '?'} (${parentPid})`
}
function requestKill(pid: number): void {
  system.requestKillProcess(pid, settings.confirmBeforeKill)
}

function toggleProcessTree(event: Event): void {
  system.setProcessTree((event.target as HTMLInputElement).checked)
}

onMounted(() => { void system.refreshProcesses() })
</script>

<template>
  <section class="view">
    <header class="view-header processes-view-header">
      <div>
        <h1>{{ t('processes') }}</h1>
        <p>{{ t('processesSubtitle') }}</p>
      </div>
      <div class="process-controls">
        <label class="search-field process-search">
          <Search :size="14" />
          <input v-model="system.processQuery" type="search" :placeholder="t('searchProcesses')" />
        </label>
        <label class="sort-control">
          <span>{{ t('sortMode') }}</span>
          <select v-model="system.processSort" :disabled="system.processTree" :aria-label="t('sortMode')">
            <option value="memory">{{ t('sortByMemory') }}</option>
            <option value="cpu">{{ t('sortByCpu') }}</option>
            <option value="pid">{{ t('sortByPid') }}</option>
            <option value="name">{{ t('sortByName') }}</option>
            <option value="started">{{ t('sortByStarted') }}</option>
          </select>
        </label>
        <label class="tree-toggle">
          <ListTree :size="14" />
          <span>{{ t('processTree') }}</span>
          <input type="checkbox" :checked="system.processTree" @change="toggleProcessTree" />
        </label>
        <button class="ghost-button refresh-button" :disabled="system.loadingProcesses" @click="system.refreshProcesses()">
          <RefreshCw :size="13" />{{ t('refresh') }}
        </button>
      </div>
    </header>

    <p v-if="system.error" class="error-banner">{{ system.error }}</p>

    <div class="section-heading">
      <h2>{{ t('processes') }}</h2>
      <div class="toolbar-actions">
        <span class="count">{{ system.filteredProcesses.length }}</span>
        <span class="count">{{ t('totalMemory') }} {{ system.processesMemory }}</span>
      </div>
    </div>

    <ul v-if="system.visibleProcesses.length" class="data-list process-list">
      <li v-for="process in system.visibleProcesses" :key="process.pid" class="data-row process-row">
        <span class="tree-indent" :style="{ paddingLeft: `${(system.processDepths.get(process.pid) ?? 0) * 14}px` }">
          <strong>{{ process.name }}</strong>
          <code>{{ process.command ?? process.executable ?? '' }}</code>
        </span>
        <span class="cell"><small>{{ t('pid') }}</small>{{ process.pid }}</span>
        <span class="cell"><small>{{ t('parentProcess') }}</small>{{ parentLabel(process.parentPid) }}</span>
        <span class="cell"><small>{{ t('memory') }}</small>{{ formatBytes(process.memoryBytes) }}</span>
        <span class="cell"><small>{{ t('started') }}</small>{{ formatUptime(process.startedAt) }}</span>
        <button
          class="ghost-button"
          :class="{ 'danger-button': system.pendingKill === `process:${process.pid}` }"
          :title="t('killProcess')"
          @click="requestKill(process.pid)"
        >
          <Skull :size="13" />
          {{ system.pendingKill === `process:${process.pid}` ? t('confirmKillProcess') : t('killProcess') }}
        </button>
      </li>
    </ul>
    <p v-else-if="system.processes.length" class="quiet">{{ t('noProcessMatches') }}</p>
    <p v-else class="quiet">{{ t('noProcesses') }}</p>
    <p v-if="system.truncatedProcesses > 0" class="quiet">{{ t('truncatedProcesses') }}</p>

    <button v-if="system.pendingKill" class="link-button" @click="system.cancelPendingKill()">{{ t('cancel') }}</button>
  </section>
</template>
