<script setup lang="ts">
import { RefreshCw, Search, Unplug } from 'lucide-vue-next'
import { onMounted } from 'vue'
import { useI18n } from '../i18n'
import { useSettingsStore } from '../stores/settings'
import { useSystemStore } from '../stores/system'
import { useWorkbenchStore } from '../stores/workbench'

const { t } = useI18n()
const system = useSystemStore()
const settings = useSettingsStore()
const workbench = useWorkbenchStore()

function matchingService(port: number): string | undefined {
  return workbench.services.find((service) => service.port === port)?.name
}

function requestFree(port: number): void {
  system.requestFreePort(port, settings.confirmBeforeKill)
}

onMounted(() => { void system.refreshPorts() })
</script>

<template>
  <section class="view">
    <header class="view-header">
      <div>
        <p class="eyebrow">{{ t('ports') }}</p>
        <h1>{{ t('listeningPorts') }}</h1>
        <p>{{ t('portsSubtitle') }}</p>
      </div>
      <div class="header-actions">
        <label class="search-field">
          <Search :size="14" />
          <input v-model="system.portQuery" type="search" :placeholder="t('searchPorts')" />
        </label>
        <button class="ghost-button" :disabled="system.loadingPorts" @click="system.refreshPorts()">
          <RefreshCw :size="13" />{{ t('refresh') }}
        </button>
      </div>
    </header>

    <p v-if="system.error" class="error-banner">{{ system.error }}</p>

    <div class="section-heading">
      <h2>{{ t('listeningPorts') }}</h2>
      <span class="count">{{ system.filteredPorts.length }}</span>
    </div>

    <ul v-if="system.filteredPorts.length" class="data-list port-table">
      <li v-for="port in system.filteredPorts" :key="`${port.address}:${port.port}`" class="data-row port-row">
        <span class="cell"><small>{{ t('port') }}</small><code>:{{ port.port }}</code></span>
        <span class="cell"><small>{{ t('address') }}</small>{{ port.address }}</span>
        <span class="cell"><small>{{ t('owningProcess') }}</small>{{ port.processName ?? t('unknownProcess') }}<small v-if="matchingService(port.port)" class="port-owner-tag">{{ t('usedBy') }} {{ matchingService(port.port) }}</small></span>
        <span class="cell"><small>{{ t('pid') }}</small>{{ port.pid ?? '—' }}</span>
        <button
          class="ghost-button"
          :class="{ 'danger-button': system.pendingKill === `port:${port.port}` }"
          :title="t('freePort')"
          :disabled="port.pid === undefined"
          @click="requestFree(port.port)"
        >
          <Unplug :size="13" />
          {{ system.pendingKill === `port:${port.port}` ? t('confirmFreePort') : t('freePort') }}
        </button>
      </li>
    </ul>
    <p v-else-if="system.ports.length" class="quiet">{{ t('noPortMatches') }}</p>
    <p v-else class="quiet">{{ t('noPorts') }}</p>

    <button v-if="system.pendingKill" class="link-button" @click="system.cancelPendingKill()">{{ t('cancel') }}</button>
  </section>
</template>
