<script setup lang="ts">
import { FolderOpen, X } from 'lucide-vue-next'
import { computed, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import LogPanel from '../components/LogPanel.vue'
import ProjectContextPanel from '../components/ProjectContextPanel.vue'
import ServiceEditor from '../components/ServiceEditor.vue'
import ServiceList from '../components/ServiceList.vue'
import { useServiceWorkspace } from '../composables/useServiceWorkspace'
import { useI18n } from '../i18n'

const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const {
  store, editing, editorOpen, batchError,
  addService, editService, closeEditor, saveService, importSuggestion, runBatch, dismiss,
} = useServiceWorkspace()

const project = computed(() => store.activeProject)
const activeError = computed(() => batchError.value ?? store.error)

// Keeps the route and the active project in sync in both directions.
watch(
  () => [route.params.id, store.projects.length] as const,
  async () => {
    const id = typeof route.params.id === 'string' ? route.params.id : undefined
    if (!id || id === store.activeProjectId) return
    const target = store.projects.find((item) => item.id === id)
    if (target) await store.activateProject(target)
  },
  { immediate: true },
)
watch(() => store.activeProjectId, (id) => {
  if (id && route.params.id !== id) void router.replace(`/projects/${id}`)
})
</script>

<template>
  <section class="view">
    <header class="view-header">
      <div>
        <p class="eyebrow">{{ t('projectWorkspace') }}</p>
        <h1>{{ project?.name ?? t('projects') }}</h1>
        <p>{{ project?.path ?? t('projectSubtitle') }}</p>
      </div>
      <button class="primary" :disabled="store.busy" @click="store.chooseProject">
        <FolderOpen :size="16" />{{ t('openProject') }}
      </button>
    </header>

    <p v-if="activeError" class="error-banner">
      {{ activeError }}
      <button class="icon-button banner-close" :aria-label="t('cancel')" @click="dismiss"><X :size="13" /></button>
    </p>

    <div v-if="project" class="workspace-grid">
      <ProjectContextPanel
        :context="store.activeContext"
        :scanning="store.scanning"
        @rescan="store.rescan()"
      />
      <ServiceList
        :services="store.services"
        :states="store.serviceStates"
        :suggestions="store.suggestedServices"
        :busy="store.busy"
        @add="addService"
        @edit="editService"
        @start="store.startService($event)"
        @stop="store.stopService($event)"
        @restart="store.restartService($event)"
        @remove="store.deleteService($event)"
        @import="importSuggestion"
        @startAll="runBatch('start')"
        @stopAll="runBatch('stop')"
      />
      <LogPanel
        v-model="store.logServiceFilter"
        :logs="store.visibleLogs"
        :services="store.services"
        @clear="store.clearLogs()"
      />
    </div>

    <button v-else class="empty-panel" :disabled="store.busy" @click="store.chooseProject">
      <FolderOpen :size="28" />
      <strong>{{ t('noProject') }}</strong>
      <span>{{ t('noProjectHint') }}</span>
    </button>

    <ServiceEditor
      v-if="editorOpen && editing"
      :service="editing"
      :services="store.services"
      @close="closeEditor"
      @save="saveService"
    />
  </section>
</template>
