<script setup lang="ts">
import { Boxes, Container, FolderGit2, FolderOpen, GitBranch, Network, Play, Radio, RefreshCw, Server, TerminalSquare } from 'lucide-vue-next'
import { computed } from 'vue'
import MetricItem from '../components/MetricItem.vue'
import { useI18n } from '../i18n'
import { useWorkbenchStore } from '../stores/workbench'

const store = useWorkbenchStore()
const { t } = useI18n()

const runningServices = computed(() =>
  store.services.filter((service) => store.stateOf(service.id).status === 'running'))
const stack = computed(() => {
  const context = store.activeContext
  if (!context) return []
  return [...new Set([...context.languages, ...context.frameworks, ...context.packageManagers])]
})
const metrics = computed(() => [
  { label: t('projects'), value: store.projects.length, tone: 'purple' as const, icon: FolderGit2 },
  { label: t('runningServices'), value: runningServices.value.length, tone: 'green' as const, icon: Server },
  { label: t('listeningPorts'), value: store.ports.length, tone: 'indigo' as const, icon: Network },
  { label: t('docker'), value: store.activeContext?.composeServices.length ?? 0, tone: 'blue' as const, icon: Container },
  { label: 'MCP', value: 0, tone: 'amber' as const, icon: Boxes },
])
</script>

<template>
  <section class="view overview-view">
    <header class="overview-hero">
      <div class="hero-copy">
        <p class="eyebrow">{{ t('localWorkspace') }} <span class="eyebrow-separator">/</span> {{ t('localOnly') }}</p>
        <h1>{{ t('greeting') }}</h1>
        <p>{{ t('overviewSubtitle') }}</p>
      </div>
      <div class="hero-actions">
        <span class="ready-state"><span class="status-dot"></span>{{ t('ready') }}</span>
        <button class="primary" :disabled="store.busy" @click="store.chooseProject">
          <FolderOpen :size="16" />{{ t('openProject') }}
        </button>
      </div>
    </header>

    <p v-if="store.error" class="error-banner">{{ store.error }}</p>

    <div class="metric-strip" aria-label="Workspace summary">
      <MetricItem v-for="metric in metrics" :key="metric.label" :label="metric.label" :value="metric.value" :tone="metric.tone">
        <component :is="metric.icon" :size="18" />
      </MetricItem>
    </div>

    <div class="overview-primary-grid">
      <section class="detail-panel overview-panel">
        <div class="panel-heading">
          <h2><FolderGit2 :size="16" />{{ t('recentProjects') }}<span class="count">{{ store.projects.length }}</span></h2>
          <RouterLink class="panel-link" to="/projects">{{ t('viewAll') }}</RouterLink>
        </div>
        <div v-if="store.projects.length" class="project-list">
          <RouterLink v-for="project in store.projects.slice(0, 5)" :key="project.id" :to="`/projects/${project.id}`">
            <span class="project-icon">{{ project.name.slice(0, 1).toUpperCase() }}</span>
            <span class="project-list-main"><strong>{{ project.name }}</strong><small>{{ project.path }}</small></span>
            <time>{{ project.lastOpenedAt ? new Date(project.lastOpenedAt).toLocaleDateString() : t('newProject') }}</time>
            <span class="row-action">{{ t('open') }} <span aria-hidden="true">↗</span></span>
          </RouterLink>
        </div>
        <button v-else class="empty-panel" :disabled="store.busy" @click="store.chooseProject">
          <FolderOpen :size="24" />
          <strong>{{ t('openFirstProject') }}</strong>
          <span>{{ t('openFirstProjectHint') }}</span>
        </button>
      </section>

      <section class="detail-panel overview-panel">
        <div class="panel-heading">
          <h2><Play :size="16" />{{ t('runningServices') }}<span class="count">{{ runningServices.length }}</span></h2>
          <RouterLink class="panel-link" to="/services">{{ t('viewAll') }}</RouterLink>
        </div>
        <div v-if="runningServices.length" class="service-list overview-service-list">
          <div v-for="service in runningServices" :key="service.id" class="overview-service-row">
            <span class="status-running service-status"><span class="status-dot"></span>{{ t('running') }}</span>
            <span class="service-main"><strong>{{ service.name }}</strong><small>{{ service.command }}</small></span>
            <code>:{{ service.port ?? '—' }}</code>
            <RouterLink class="row-action" to="/services">{{ t('open') }} <span aria-hidden="true">↗</span></RouterLink>
          </div>
        </div>
        <div v-else class="empty-inline">
          <span class="empty-glyph"><TerminalSquare :size="18" /></span>
          <span><strong>{{ t('noServicesRunning') }}</strong><small>{{ t('noServicesHint') }}</small></span>
          <RouterLink class="ghost-button" to="/services">{{ t('configureService') }}</RouterLink>
        </div>
      </section>
    </div>

    <div class="overview-secondary-grid">
      <section class="detail-panel overview-panel ports-panel">
        <div class="panel-heading">
          <h2><Radio :size="16" />{{ t('listeningPorts') }}<span class="count">{{ store.ports.length }}</span></h2>
          <button class="icon-button" :title="t('refreshPorts')" @click="store.refreshPorts()"><RefreshCw :size="14" /></button>
        </div>
        <div class="developer-table">
          <div class="table-header"><span>{{ t('port') }}</span><span>{{ t('owningProcess') }}</span><span>{{ t('address') }}</span><span>{{ t('status') }}</span></div>
          <div v-for="port in store.ports.slice(0, 6)" :key="`${port.address}:${port.port}`" class="table-row">
            <code>:{{ port.port }}</code><span>{{ port.processName ?? t('unknownProcess') }}</span><code>{{ port.address }}</code><span class="table-status"><span class="status-dot"></span>{{ t('listening') }}</span>
          </div>
          <p v-if="!store.ports.length" class="quiet table-empty">{{ t('noPorts') }}</p>
        </div>
      </section>

      <section class="detail-panel overview-panel context-overview-panel">
        <div class="panel-heading">
          <h2><GitBranch :size="16" />{{ t('currentContext') }}</h2>
          <RouterLink v-if="store.activeProject" class="panel-link" :to="`/projects/${store.activeProject.id}`">{{ t('viewAll') }}</RouterLink>
        </div>
        <div v-if="store.activeProject" class="context-summary">
          <div class="context-summary-title"><span class="project-icon"><FolderGit2 :size="16" /></span><span><strong>{{ store.activeProject.name }}</strong><small>{{ store.activeProject.path }}</small></span></div>
          <div class="chip-row"><span v-for="value in stack" :key="value" class="chip">{{ value }}</span><span v-if="!stack.length" class="quiet">{{ t('noStackDetected') }}</span></div>
        </div>
        <div v-else class="empty-inline compact-empty"><span class="empty-glyph"><FolderGit2 :size="18" /></span><span><strong>{{ t('noProject') }}</strong><small>{{ t('noProjectHint') }}</small></span></div>
      </section>
    </div>
  </section>
</template>
