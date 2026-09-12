<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from '../i18n'

defineOptions({ name: 'JsonTree' })

const props = withDefaults(defineProps<{ value: unknown; label?: string; depth?: number }>(), { depth: 0 })
const { locale } = useI18n()
const isZh = computed(() => locale.value === 'zh-CN')
const isBranch = computed(() => props.value !== null && typeof props.value === 'object')
const entries = computed(() => Array.isArray(props.value) ? props.value.map((value, index) => [String(index), value] as const) : Object.entries(props.value as Record<string, unknown>))
const branchSize = computed(() => entries.value.length)

function preview(value: unknown): string {
  if (value === null) return 'null'
  if (typeof value === 'string') return JSON.stringify(value)
  if (typeof value === 'object') return Array.isArray(value) ? `[${Object.keys(value).length}]` : `{${Object.keys(value as object).length}}`
  return String(value)
}
</script>

<template>
  <details v-if="isBranch" class="json-tree-node" :open="props.depth < 1">
    <summary><span class="json-tree-chevron">›</span><strong v-if="label">{{ label }}</strong><span class="json-tree-preview">{{ preview(value) }}</span><small>{{ branchSize }} {{ Array.isArray(value) ? (isZh ? '项' : 'items') : (isZh ? '个 Key' : 'keys') }}</small></summary>
    <div class="json-tree-children"><JsonTree v-for="([key, child], index) in entries" :key="`${key}-${index}`" :label="key" :value="child" :depth="props.depth + 1" /></div>
  </details>
  <div v-else class="json-tree-leaf"><strong v-if="label">{{ label }}</strong><code :class="`json-tree-${value === null ? 'null' : typeof value}`">{{ preview(value) }}</code></div>
</template>

<style scoped>
.json-tree-output { max-height: 420px; overflow: auto; padding: 8px 0; border: 1px solid var(--border); border-radius: 6px; background: #0b0e13; color: var(--foreground); font: 11px/1.7 "Cascadia Code", Consolas, monospace; }
.json-tree-node { margin: 0; }.json-tree-node summary { display: flex; align-items: center; gap: 8px; min-height: 28px; padding: 2px 10px; color: var(--foreground); cursor: pointer; list-style: none; }.json-tree-node summary::-webkit-details-marker { display: none; }.json-tree-node summary:hover { background: rgb(147 136 245 / .1); }.json-tree-chevron { display: inline-block; width: 12px; color: var(--accent); font-size: 17px; line-height: 1; transform: rotate(0deg); transition: transform .14s; }.json-tree-node[open] > summary .json-tree-chevron { transform: rotate(90deg); }.json-tree-node summary strong, .json-tree-leaf strong { color: #b3a8ff; font-weight: 500; }.json-tree-preview { color: #8993a7; }.json-tree-node summary small { margin-left: auto; color: var(--faint); font-size: 9px; }.json-tree-children { margin-left: 18px; border-left: 1px solid var(--border); }.json-tree-leaf { display: flex; align-items: baseline; gap: 8px; min-height: 26px; padding: 2px 10px 2px 30px; }.json-tree-string { color: #a8dfa9; }.json-tree-number { color: #f0c87a; }.json-tree-boolean { color: #7ec8f5; }.json-tree-null { color: #ef9aac; }
</style>
