<script setup lang="ts">
import type { DevService, ServiceState, SuggestedService } from '@dev-workbench/shared'
import { ListPlus, Play, Plus, RotateCw, Square, TerminalSquare, Trash2, Pencil, Download } from 'lucide-vue-next'
import { ref } from 'vue'
import { useI18n } from '../i18n'

const props = defineProps<{
  services: DevService[]
  states: Record<string, ServiceState>
  suggestions: SuggestedService[]
  busy?: boolean | undefined
}>()

const emit = defineEmits<{
  start: [service: DevService]
  stop: [serviceId: string]
  restart: [service: DevService]
  edit: [service: DevService]
  remove: [serviceId: string]
  add: []
  startAll: []
  stopAll: []
  import: [suggestion: SuggestedService]
}>()

const { t } = useI18n()
const pendingDelete = ref<string>()

function statusOf(service: DevService): ServiceState {
  return props.states[service.id] ?? { serviceId: service.id, status: 'stopped' }
}
function statusLabel(status: ServiceState['status']): string {
  return t(status)
}
function isBusy(service: DevService): boolean {
  const status = statusOf(service).status
  return status === 'starting' || status === 'stopping'
}
function commandLine(service: DevService): string {
  return [service.command, ...(service.args ?? [])].join(' ')
}
function imported(name: string): boolean {
  return props.services.some((service) => service.name === name)
}
function confirmDelete(serviceId: string): void {
  if (pendingDelete.value === serviceId) {
    pendingDelete.value = undefined
    emit('remove', serviceId)
    return
  }
  pendingDelete.value = serviceId
}
</script>

<template>
  <section class="detail-panel services-panel">
    <div class="panel-heading">
      <h2><TerminalSquare :size="16" />{{ t('services') }}<span class="count">{{ services.length }}</span></h2>
      <div class="toolbar-actions">
        <button class="ghost-button" :disabled="busy || !services.length" @click="emit('startAll')">
          <Play :size="13" />{{ t('startAll') }}
        </button>
        <button class="ghost-button" :disabled="busy || !services.length" @click="emit('stopAll')">
          <Square :size="13" />{{ t('stopAll') }}
        </button>
        <button class="primary" :disabled="busy" @click="emit('add')">
          <Plus :size="14" />{{ t('addService') }}
        </button>
      </div>
    </div>

    <p v-if="!services.length" class="quiet services-empty">{{ t('noServicesDefined') }}</p>

    <ul v-else class="service-list">
      <li v-for="service in services" :key="service.id" class="service-row">
        <span class="status-badge" :class="`status-${statusOf(service).status}`">
          <span class="status-dot" />{{ statusLabel(statusOf(service).status) }}
        </span>
        <div class="service-main">
          <strong>{{ service.name }}</strong>
          <code>{{ commandLine(service) }}</code>
          <small v-if="service.cwd">{{ service.cwd }}</small>
        </div>
        <div class="service-meta">
          <span v-if="service.port" class="chip">:{{ service.port }}</span>
          <span v-if="statusOf(service).pid" class="chip">pid {{ statusOf(service).pid }}</span>
        </div>
        <div class="service-actions">
          <button
            v-if="statusOf(service).status !== 'running'"
            class="icon-button"
            :title="t('start')"
            :disabled="busy || isBusy(service)"
            @click="emit('start', service)"
          >
            <Play :size="14" />
          </button>
          <template v-else>
            <button class="icon-button" :title="t('stop')" :disabled="busy" @click="emit('stop', service.id)">
              <Square :size="14" />
            </button>
            <button class="icon-button" :title="t('restart')" :disabled="busy" @click="emit('restart', service)">
              <RotateCw :size="14" />
            </button>
          </template>
          <button class="icon-button" :title="t('editService')" :disabled="busy" @click="emit('edit', service)">
            <Pencil :size="14" />
          </button>
          <button
            class="icon-button"
            :class="{ 'danger-button': pendingDelete === service.id }"
            :title="t('deleteService')"
            :disabled="busy"
            @click="confirmDelete(service.id)"
          >
            <Trash2 :size="14" />
            <span v-if="pendingDelete === service.id" class="confirm-label">{{ t('confirmDelete') }}</span>
          </button>
        </div>
      </li>
    </ul>

    <div v-if="suggestions.length" class="suggestion-block">
      <span class="stack-label"><ListPlus :size="12" />{{ t('suggestedServices') }}</span>
      <div class="suggestion-list">
        <div v-for="suggestion in suggestions" :key="`${suggestion.source}:${suggestion.name}`" class="suggestion">
          <span>
            <strong>{{ suggestion.name }}</strong>
            <small>{{ suggestion.command }} {{ suggestion.args.join(' ') }}</small>
          </span>
          <button
            class="ghost-button"
            :disabled="busy || imported(suggestion.name)"
            @click="emit('import', suggestion)"
          >
            <Download :size="12" />{{ t('importSuggestion') }}
          </button>
        </div>
      </div>
    </div>
  </section>
</template>
