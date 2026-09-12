<script setup lang="ts">
import { FolderGit2, Play } from 'lucide-vue-next'
import { computed, onMounted } from 'vue'
import LogPanel from '../components/LogPanel.vue'
import ServiceEditor from '../components/ServiceEditor.vue'
import ServiceList from '../components/ServiceList.vue'
import { useServiceWorkspace } from '../composables/useServiceWorkspace'
import { useI18n } from '../i18n'

const { t } = useI18n()
const {
  store, editing, editorOpen, batchError,
  addService, editService, closeEditor, saveService, importSuggestion, runBatch, dismiss,
} = useServiceWorkspace()

const error = computed(() => batchError.value ?? store.error)

async function selectProject(event: Event): Promise<void> {
  const id = (event.target as HTMLSelectElement).value
  const project = store.projects.find((item) => item.id === id)
  if (project) await store.activateProject(project)
}

onMounted(() => { store.refreshRunning() })
</script>

<template>
  <section class="view">
    <header class="view-header">
      <div>
        <p class="eyebrow">{{ t('services') }}</p>
        <h1>{{ store.activeProject?.name ?? t('services') }}</h1>
        <p>{{ t('servicesSubtitle') }}</p>
      </div>
      <div class="header-actions">
        <label class="field inline-field">
          <span>{{ t('activeProject') }}</span>
          <select class="filter-select" :value="store.activeProjectId ?? ''" @change="selectProject">
            <option value="" disabled>{{ t('noProject') }}</option>
            <option v-for="project in store.projects" :key="project.id" :value="project.id">{{ project.name }}</option>
          </select>
        </label>
        <button class="primary" :disabled="store.busy" @click="store.chooseProject">
          <FolderGit2 :size="16" />{{ t('openProject') }}
        </button>
      </div>
    </header>

    <p v-if="error" class="error-banner">{{ error }}</p>

    <div v-if="store.activeProject" class="workspace-grid">
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
      <Play :size="28" />
      <strong>{{ t('noProject') }}</strong>
      <span>{{ t('projectSubtitle') }}</span>
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
