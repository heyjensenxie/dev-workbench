<script setup lang="ts">
import { ExternalLink, FolderGit2, FolderOpen, RefreshCw, Trash2 } from 'lucide-vue-next'
import { onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { useI18n } from '../i18n'
import { useWorkbenchStore } from '../stores/workbench'

const store = useWorkbenchStore()
const router = useRouter()
const { t } = useI18n()
const pendingRemove = ref<string>()

function openProject(id: string): void {
  void router.push(`/projects/${id}`)
}

async function removeProject(id: string): Promise<void> {
  if (pendingRemove.value !== id) {
    pendingRemove.value = id
    return
  }
  const project = store.projects.find((item) => item.id === id)
  pendingRemove.value = undefined
  if (project) await store.removeProject(project)
}

onMounted(() => { if (!store.projects.length) void store.refreshProjects() })
</script>

<template>
  <section class="view">
    <header class="view-header">
      <div>
        <p class="eyebrow">{{ t('projects') }}</p>
        <h1>{{ t('projects') }}</h1>
        <p>{{ t('projectsSubtitle') }}</p>
      </div>
      <div class="header-actions">
        <button class="ghost-button" :disabled="store.busy" :title="t('refreshProjects')" @click="store.refreshProjects()">
          <RefreshCw :size="13" />{{ t('refresh') }}
        </button>
        <button class="primary" :disabled="store.busy" @click="store.chooseProject">
          <FolderOpen :size="16" />{{ t('addProject') }}
        </button>
      </div>
    </header>

    <p v-if="store.error" class="error-banner">{{ store.error }}</p>

    <div v-if="store.projects.length" class="managed-project-list">
      <article v-for="project in store.projects" :key="project.id" class="managed-project-row">
        <button class="managed-project-main" @click="openProject(project.id)">
          <span class="project-icon"><FolderGit2 :size="16" /></span>
          <span>
            <strong>{{ project.name }}</strong>
            <small>{{ project.path }}</small>
            <time>{{ project.lastOpenedAt ? new Date(project.lastOpenedAt).toLocaleString() : t('newProject') }}</time>
          </span>
        </button>
        <div class="service-actions">
          <button class="icon-button" :title="t('revealProject')" :disabled="store.busy" @click="store.revealProject(project)">
            <ExternalLink :size="14" />
          </button>
          <button
            class="icon-button"
            :class="{ 'danger-button': pendingRemove === project.id }"
            :title="t('removeProject')"
            :disabled="store.busy"
            @click="removeProject(project.id)"
          >
            <Trash2 :size="14" />
            <span v-if="pendingRemove === project.id" class="confirm-label">{{ t('confirmRemoveProject') }}</span>
          </button>
        </div>
      </article>
    </div>
    <button v-else class="empty-panel" :disabled="store.busy" @click="store.chooseProject">
      <FolderOpen :size="28" />
      <strong>{{ t('openFirstProject') }}</strong>
      <span>{{ t('openFirstProjectHint') }}</span>
    </button>

    <p class="project-safety-note">{{ t('projectRemovalHint') }}</p>
  </section>
</template>
