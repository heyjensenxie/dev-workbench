<script setup lang="ts">
import type { ProjectContext } from '@dev-workbench/shared'
import { FileCode2, GitBranch, RefreshCw } from 'lucide-vue-next'
import { computed, ref } from 'vue'
import { useI18n } from '../i18n'

const props = defineProps<{
  context?: ProjectContext | undefined
  scanning?: boolean | undefined
}>()
defineEmits<{ rescan: [] }>()

const { t } = useI18n()
const showFiles = ref(false)

const groups = computed(() => {
  const context = props.context
  if (!context) return []
  return [
    { key: 'languages', label: t('languages'), values: context.languages },
    { key: 'frameworks', label: t('frameworks'), values: context.frameworks },
    { key: 'packageManagers', label: t('packageManagers'), values: context.packageManagers },
  ].filter((group) => group.values.length > 0)
})

const scripts = computed(() => props.context?.scripts ?? [])
const detectedFiles = computed(() => props.context?.detectedFiles ?? [])
const scannedAt = computed(() =>
  props.context ? new Date(props.context.scannedAt).toLocaleTimeString() : '')
</script>

<template>
  <section class="detail-panel context-panel">
    <div class="panel-heading">
      <h2><FileCode2 :size="16" />{{ t('projectContext') }}</h2>
      <button class="ghost-button" :disabled="scanning" @click="$emit('rescan')">
        <RefreshCw :size="13" />{{ scanning ? t('rescanning') : t('rescan') }}
      </button>
    </div>

    <p v-if="!context" class="quiet">{{ t('noProjectHint') }}</p>

    <template v-else>
      <p v-if="context.description" class="context-description">{{ context.description }}</p>

      <div class="chip-row">
        <span v-if="context.vcs?.branch" class="chip"><GitBranch :size="11" />{{ context.vcs.branch }}</span>
        <span v-if="context.monorepo" class="chip">{{ t('monorepo') }}</span>
        <span v-if="scannedAt" class="chip quiet-chip">{{ scannedAt }}</span>
      </div>

      <div v-for="group in groups" :key="group.key" class="stack-group">
        <span class="stack-label">{{ group.label }}</span>
        <div class="chip-row">
          <span v-for="value in group.values" :key="value" class="chip">{{ value }}</span>
        </div>
      </div>
      <p v-if="!groups.length" class="quiet">{{ t('noStackDetected') }}</p>

      <div v-if="context.composeServices.length" class="stack-group">
        <span class="stack-label">{{ t('composeServices') }}</span>
        <div class="chip-row">
          <span v-for="service in context.composeServices" :key="service" class="chip">{{ service }}</span>
        </div>
      </div>

      <div v-if="scripts.length" class="stack-group">
        <span class="stack-label">{{ t('scripts') }}</span>
        <ul class="script-list">
          <li v-for="script in scripts" :key="`${script.source}:${script.name}`">
            <code>{{ script.name }}</code>
            <span>{{ script.command }}</span>
          </li>
        </ul>
      </div>

      <button class="link-button" @click="showFiles = !showFiles">
        {{ t('detectedFiles') }} · {{ detectedFiles.length }}
      </button>
      <ul v-if="showFiles" class="file-list">
        <li v-for="file in detectedFiles" :key="file.path">
          <code>{{ file.path }}</code><small>{{ file.kind }}</small>
        </li>
      </ul>
    </template>
  </section>
</template>
