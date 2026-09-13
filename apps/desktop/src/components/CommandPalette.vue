<script setup lang="ts">
import { fuzzyMatch } from '@dev-workbench/command'
import { Command, Search } from 'lucide-vue-next'
import { computed, nextTick, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { workbench } from '../services/workbench'
import { useI18n, type MessageKey } from '../i18n'
import { useWorkbenchStore } from '../stores/workbench'

const props = defineProps<{ open: boolean }>()
const emit = defineEmits<{ 'update:open': [value: boolean] }>()
const { t } = useI18n()
const store = useWorkbenchStore()
const router = useRouter()
/** Commands that take no argument, so the palette can run them directly. */
const inputFreeCommands = new Set(['process.list', 'port.list', 'service.listRunning', 'service.stopEverything', 'vault.lock'])
const utilityCommands = new Set(['utility.sql.open', 'utility.mybatis.restore.open', 'utility.api.open'])
const databaseCommands = new Set(['database.open'])
const vaultCommands = new Set(['vault.open'])
const fileCommands = new Set(['file.open'])
const paletteCommands = new Set(['project.open', 'project.refresh', 'service.startAll', 'service.stopAll', ...inputFreeCommands, ...utilityCommands, ...databaseCommands, ...vaultCommands, ...fileCommands])
/** Localized labels for registry commands, which are authored in English. */
const commandLabels: Record<string, { title: MessageKey; category?: MessageKey }> = {
  'project.open': { title: 'commandProjectOpen', category: 'categoryProject' },
  'project.refresh': { title: 'commandProjectRefresh', category: 'categoryProject' },
  'service.startAll': { title: 'commandServiceStartAll', category: 'categoryServices' },
  'service.stopAll': { title: 'commandServiceStopAll', category: 'categoryServices' },
  'service.stopEverything': { title: 'commandServiceStopEverything', category: 'categoryServices' },
  'service.listRunning': { title: 'commandServiceListRunning', category: 'categoryServices' },
  'process.list': { title: 'commandProcessList', category: 'categorySystem' },
  'port.list': { title: 'commandPortList', category: 'categorySystem' },
  'utility.sql.open': { title: 'commandSqlOpen', category: 'categoryUtilitiesDatabase' },
  'utility.mybatis.restore.open': { title: 'commandMybatisOpen', category: 'categoryUtilitiesDatabase' },
  'utility.api.open': { title: 'commandApiOpen', category: 'categoryUtilitiesApi' },
  'file.open': { title: 'commandFileOpen', category: 'categoryFile' },
  'database.open': { title: 'commandDatabaseOpen', category: 'categoryDatabase' },
  // Only two vault commands exist, and neither can return a secret.
  'vault.open': { title: 'commandVaultOpen', category: 'categorySecurity' },
  'vault.lock': { title: 'commandVaultLock', category: 'categorySecurity' },
}
function commandTitle(id: string, fallback: string): string {
  const keys = commandLabels[id]
  return keys ? t(keys.title) : fallback
}
function commandCategory(id: string, fallback: string | undefined): string | undefined {
  const keys = commandLabels[id]
  return keys?.category ? t(keys.category) : fallback
}
const query = ref('')
const input = ref<HTMLInputElement>()
const selectedIndex = ref(0)
const results = computed(() => {
  const generic = workbench.commands.list()
    .filter((item) => paletteCommands.has(item.id))
    .map((item) => ({ id: item.id, title: commandTitle(item.id, item.title), category: commandCategory(item.id, item.category) }))
  const dynamic = store.services.flatMap((service) => {
    const status = store.stateOf(service.id).status
    const actions = status === 'running' ? ['service.restart', 'service.stop'] : ['service.start']
    return actions.map((action) => ({
      id: `${action}:${service.id}`,
      title: `${action === 'service.start' ? t('start') : action === 'service.stop' ? t('stop') : t('restart')} ${service.name}`,
      category: t('services'),
    }))
  })
  return [...generic, ...dynamic]
    .filter((item) => fuzzyMatch(query.value, `${item.title} ${item.id} ${item.category ?? ''}`))
    .slice(0, 12)
})
watch(() => props.open, async (open) => { if (open) { query.value = ''; selectedIndex.value = 0; await nextTick(); input.value?.focus() } })
watch(query, () => { selectedIndex.value = 0 })
function moveSelection(direction: 1 | -1): void {
  if (!results.value.length) return
  selectedIndex.value = (selectedIndex.value + direction + results.value.length) % results.value.length
}
function executeSelected(): void {
  const item = results.value[selectedIndex.value]
  if (item) void execute(item.id)
}
async function execute(id: string): Promise<void> {
  if (id === 'project.open') await store.chooseProject()
  else if (id === 'project.refresh') await store.rescan()
  else if (id === 'service.startAll') await store.startAll()
  else if (id === 'service.stopAll') await store.stopAll()
  else if (id.startsWith('service.start:')) {
    const service = store.services.find((item) => item.id === id.slice('service.start:'.length))
    if (service) await store.startService(service)
  } else if (id.startsWith('service.stop:')) {
    await store.stopService(id.slice('service.stop:'.length))
  } else if (id.startsWith('service.restart:')) {
    const service = store.services.find((item) => item.id === id.slice('service.restart:'.length))
    if (service) await store.restartService(service)
  } else if (utilityCommands.has(id)) {
    const utilityId = await workbench.commands.execute<undefined, string>(id, undefined, workbench.commandContext)
    await router.push(utilityId === 'api' ? { name: 'api' } : { name: 'utilities', query: { tool: utilityId } })
  } else if (databaseCommands.has(id)) {
    await router.push({ name: 'database' })
  } else if (vaultCommands.has(id)) {
    await router.push({ name: 'vault' })
  } else if (fileCommands.has(id)) {
    await router.push({ name: 'files' })
  } else if (inputFreeCommands.has(id)) {
    await workbench.commands.execute(id, undefined, workbench.commandContext)
  }
  emit('update:open', false)
}
</script>
<template>
  <div v-if="open" class="palette-backdrop" role="presentation" @mousedown.self="emit('update:open', false)">
    <section class="palette" role="dialog" aria-modal="true" aria-label="Command Palette" @keydown.esc="emit('update:open', false)">
      <label class="palette-search"><Search :size="18" /><input ref="input" v-model="query" :placeholder="t('searchCommands')" @keydown.down.prevent="moveSelection(1)" @keydown.up.prevent="moveSelection(-1)" @keydown.enter.prevent="executeSelected" /></label>
      <div class="palette-results">
        <button v-for="(item, index) in results" :key="item.id" :class="{ selected: selectedIndex === index }" :aria-selected="selectedIndex === index" @mouseenter="selectedIndex = index" @click="execute(item.id)"><Command :size="15" /><span><strong>{{ item.title }}</strong><small>{{ item.category }} · {{ item.id }}</small></span><kbd v-if="selectedIndex === index">↵</kbd></button>
        <p v-if="!results.length" class="empty">{{ t('noMatchingCommands') }}</p>
      </div>
    </section>
  </div>
</template>
