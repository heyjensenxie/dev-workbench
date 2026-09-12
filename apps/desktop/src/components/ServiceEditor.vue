<script setup lang="ts">
import type { DevService, ServiceIssue } from '@dev-workbench/shared'
import { validateDevService } from '@dev-workbench/shared'
import { Plus, Trash2, X } from 'lucide-vue-next'
import { computed, reactive, ref } from 'vue'
import { useI18n } from '../i18n'

const props = defineProps<{
  service: DevService
  services: DevService[]
}>()
const emit = defineEmits<{
  close: []
  save: [service: DevService]
}>()

const { t } = useI18n()

const ISSUE_KEYS: Record<ServiceIssue, 'nameRequired' | 'nameTooLong' | 'commandRequired' | 'selfDependency'> = {
  nameRequired: 'nameRequired',
  nameTooLong: 'nameTooLong',
  commandRequired: 'commandRequired',
  selfDependency: 'selfDependency',
}

const fields = reactive({
  name: props.service.name,
  command: props.service.command,
  cwd: props.service.cwd ?? '',
  port: props.service.port === undefined ? '' : String(props.service.port),
  autoOpen: props.service.autoOpen ?? false,
})
const args = ref<Array<{ value: string }>>((props.service.args ?? []).map((value) => ({ value })))
const envRows = ref<Array<{ key: string; value: string }>>(
  Object.entries(props.service.env ?? {}).map(([key, value]) => ({ key, value })),
)
const dependencies = ref<string[]>([...(props.service.dependencies ?? [])])

function buildService(): DevService {
  const cwd = fields.cwd.trim()
  const portText = fields.port.trim()
  const port = /^\d+$/.test(portText) ? Number.parseInt(portText, 10) : Number.NaN
  return {
    id: props.service.id,
    projectId: props.service.projectId,
    name: fields.name.trim(),
    command: fields.command.trim(),
    args: args.value.map((argument) => argument.value.trim()).filter(Boolean),
    env: Object.fromEntries(
      envRows.value
        .filter((row) => row.key.trim())
        .map((row) => [row.key.trim(), row.value]),
    ),
    autoOpen: fields.autoOpen,
    dependencies: dependencies.value,
    updatedAt: props.service.updatedAt ?? 0,
    ...(cwd ? { cwd } : {}),
    ...(Number.isFinite(port) && port > 0 && port <= 65535 ? { port } : {}),
  }
}

const issues = computed(() => validateDevService(buildService()))
const dependencyOptions = computed(() => props.services.filter((service) => service.id !== props.service.id))
const preview = computed(() => {
  const service = buildService()
  return [service.command, ...service.args ?? []].filter(Boolean).join(' ')
})
const isNew = computed(() => !props.services.some((service) => service.id === props.service.id))

function addArgument(): void { args.value = [...args.value, { value: '' }] }
function removeArgument(index: number): void { args.value = args.value.filter((_, position) => position !== index) }
function addVariable(): void { envRows.value = [...envRows.value, { key: '', value: '' }] }
function removeVariable(index: number): void { envRows.value = envRows.value.filter((_, position) => position !== index) }
function toggleDependency(serviceId: string): void {
  dependencies.value = dependencies.value.includes(serviceId)
    ? dependencies.value.filter((id) => id !== serviceId)
    : [...dependencies.value, serviceId]
}
function submit(): void {
  if (issues.value.length) return
  emit('save', buildService())
}
</script>

<template>
  <div class="editor-backdrop" role="presentation" @mousedown.self="emit('close')">
    <section class="editor" role="dialog" aria-modal="true" @keydown.esc="emit('close')">
      <header class="editor-header">
        <h2>{{ isNew ? t('addService') : t('editService') }}</h2>
        <button class="icon-button" :aria-label="t('cancel')" @click="emit('close')"><X :size="16" /></button>
      </header>

      <form class="editor-body" @submit.prevent="submit">
        <div class="editor-grid">
          <label class="field">
            <span>{{ t('name') }}</span>
            <input v-model="fields.name" type="text" />
          </label>
          <label class="field">
            <span>{{ t('port') }} <small>{{ t('optional') }}</small></span>
            <input v-model="fields.port" type="text" inputmode="numeric" />
          </label>
          <label class="field field-wide">
            <span>{{ t('command') }}</span>
            <input v-model="fields.command" type="text" placeholder="pnpm" />
          </label>
          <label class="field field-wide">
            <span>{{ t('workingDirectory') }} <small>{{ t('optional') }}</small></span>
            <input v-model="fields.cwd" type="text" placeholder="./apps/web or C:\\projects\\app" />
          </label>
        </div>

        <div class="field">
          <span>{{ t('arguments') }}</span>
          <div class="kv-list">
            <div v-for="(argument, index) in args" :key="`arg-${index}`" class="kv-row single">
              <input v-model="argument.value" type="text" />
              <button type="button" class="icon-button" :aria-label="t('deleteService')" @click="removeArgument(index)">
                <Trash2 :size="13" />
              </button>
            </div>
            <button type="button" class="ghost-button" @click="addArgument"><Plus :size="13" />{{ t('arguments') }}</button>
          </div>
        </div>

        <div class="field">
          <span>{{ t('environment') }}</span>
          <div class="kv-list">
            <div v-for="(row, index) in envRows" :key="`env-${index}`" class="kv-row">
              <input v-model="row.key" type="text" :placeholder="t('key')" />
              <input v-model="row.value" type="text" :placeholder="t('value')" />
              <button type="button" class="icon-button" :aria-label="t('deleteService')" @click="removeVariable(index)">
                <Trash2 :size="13" />
              </button>
            </div>
            <button type="button" class="ghost-button" @click="addVariable"><Plus :size="13" />{{ t('addVariable') }}</button>
          </div>
        </div>

        <div v-if="dependencyOptions.length" class="field">
          <span>{{ t('dependencies') }}</span>
          <div class="dependency-list">
            <label v-for="option in dependencyOptions" :key="option.id" class="checkbox-row">
              <input
                type="checkbox"
                :checked="dependencies.includes(option.id)"
                @change="toggleDependency(option.id)"
              />
              {{ option.name }}
            </label>
          </div>
        </div>

        <label class="checkbox-row">
          <input v-model="fields.autoOpen" type="checkbox" />
          {{ t('autoOpen') }}
        </label>

        <p v-if="preview" class="command-preview"><code>{{ preview }}</code></p>

        <ul v-if="issues.length" class="issue-list">
          <li v-for="issue in issues" :key="issue">{{ t(ISSUE_KEYS[issue]) }}</li>
        </ul>

        <footer class="editor-footer">
          <button type="button" class="ghost-button" @click="emit('close')">{{ t('cancel') }}</button>
          <button type="submit" class="primary" :disabled="issues.length > 0">{{ t('save') }}</button>
        </footer>
      </form>
    </section>
  </div>
</template>
