<script setup lang="ts">
import type { DevService, ServiceLogEvent } from '@dev-workbench/shared'
import { Eraser, Pause, Play, Search, TerminalSquare } from 'lucide-vue-next'
import { computed, nextTick, ref, watch } from 'vue'
import { useI18n } from '../i18n'
import { ALL_SERVICES } from '../stores/workbench'

const props = defineProps<{
  logs: ServiceLogEvent[]
  services: DevService[]
  modelValue: string
}>()
const emit = defineEmits<{
  'update:modelValue': [value: string]
  clear: []
}>()

const { t } = useI18n()
const output = ref<HTMLPreElement>()
const paused = ref(false)
const query = ref('')
const visibleLogs = computed(() => {
  const needle = query.value.trim().toLocaleLowerCase()
  return needle ? props.logs.filter((log) => log.line.toLocaleLowerCase().includes(needle)) : props.logs
})

watch(() => visibleLogs.value.length, async () => {
  if (paused.value) return
  await nextTick()
  const element = output.value
  if (element) element.scrollTop = element.scrollHeight
})

function serviceName(serviceId: string): string {
  return props.services.find((service) => service.id === serviceId)?.name ?? serviceId.slice(0, 8)
}
function timeOf(timestamp: number): string {
  return new Date(timestamp).toLocaleTimeString()
}
function onFilterChange(event: Event): void {
  emit('update:modelValue', (event.target as HTMLSelectElement).value)
}
function severityClass(line: string): string {
  const value = line.toLocaleUpperCase()
  if (value.includes('ERROR') || value.includes('FATAL')) return 'log-error'
  if (value.includes('WARN')) return 'log-warn'
  return ''
}
</script>

<template>
  <section class="detail-panel logs-panel">
    <div class="panel-heading">
      <h2><TerminalSquare :size="16" />{{ t('logs') }}<span class="count">{{ visibleLogs.length }}</span></h2>
      <div class="toolbar-actions">
        <select class="filter-select" :value="modelValue" @change="onFilterChange">
          <option :value="ALL_SERVICES">{{ t('allServices') }}</option>
          <option v-for="service in services" :key="service.id" :value="service.id">{{ service.name }}</option>
        </select>
        <label class="log-search"><Search :size="12" /><input v-model="query" type="search" :placeholder="t('searchLogs')" /></label>
        <button class="ghost-button" :disabled="!logs.length" @click="paused = !paused">
          <Play v-if="paused" :size="13" /><Pause v-else :size="13" />{{ paused ? t('resumeLogs') : t('pauseLogs') }}
        </button>
        <button class="ghost-button" :disabled="!logs.length" @click="emit('clear')">
          <Eraser :size="13" />{{ t('clearLogs') }}
        </button>
      </div>
    </div>
    <pre ref="output"><span v-for="(log, index) in visibleLogs" :key="index" :class="[log.stream, severityClass(log.line)]"><time>{{ timeOf(log.timestamp) }}</time><b>{{ serviceName(log.serviceId) }}</b>{{ log.line }}
</span><span v-if="!visibleLogs.length" class="quiet">{{ query ? t('noMatchingLogs') : t('noLogs') }}</span></pre>
  </section>
</template>
