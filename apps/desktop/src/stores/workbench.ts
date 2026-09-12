import type { BatchOutcome } from '@dev-workbench/core'
import { serviceFromSuggestion } from '@dev-workbench/core'
import type { DevService, PortInfo, Project, ProjectContext, ServiceLogEvent, ServiceState, SuggestedService } from '@dev-workbench/shared'
import { AppError, createServiceId } from '@dev-workbench/shared'
import { open } from '@tauri-apps/plugin-dialog'
import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { subscribeToServiceExit, subscribeToServiceLogs } from '../services/nativeBridge'
import { workbench } from '../services/workbench'
import { useSettingsStore } from './settings'

export const ALL_SERVICES = 'all'

export const useWorkbenchStore = defineStore('workbench', () => {
  const projects = ref<Project[]>([])
  const ports = ref<PortInfo[]>([])
  const services = ref<DevService[]>([])
  const serviceStates = ref<Record<string, ServiceState>>({})
  const activeContext = ref<ProjectContext>()
  const activeProjectId = ref<string>()
  const logs = ref<ServiceLogEvent[]>([])
  const logServiceFilter = ref<string>(ALL_SERVICES)
  const error = ref<string>()
  const busy = ref(false)
  const scanning = ref(false)

  const activeProject = computed(() => projects.value.find(({ id }) => id === activeProjectId.value))
  const visibleLogs = computed(() =>
    logServiceFilter.value === ALL_SERVICES
      ? logs.value
      : logs.value.filter((log) => log.serviceId === logServiceFilter.value))
  const runningCount = computed(() =>
    services.value.filter((service) => stateOf(service.id).status === 'running').length)
  const suggestedServices = computed(() => activeContext.value?.suggestedServices ?? [])

  function stateOf(serviceId: string): ServiceState {
    return serviceStates.value[serviceId] ?? { serviceId, status: 'stopped' }
  }

  function applyStates(states: ServiceState[]): void {
    const next = { ...serviceStates.value }
    for (const state of states) next[state.serviceId] = state
    serviceStates.value = next
  }

  function reportError(cause: unknown): void {
    error.value = AppError.from(cause).message
  }

  function dismissError(): void {
    error.value = undefined
  }

  async function initialize(): Promise<void> {
    busy.value = true
    try {
      const [loadedProjects, loadedPorts, running] = await Promise.all([
        workbench.projects.list(),
        workbench.commands.execute<undefined, PortInfo[]>('port.list', undefined, workbench.commandContext),
        workbench.commands.execute<undefined, ServiceState[]>('service.listRunning', undefined, workbench.commandContext),
      ])
      projects.value = loadedProjects
      ports.value = loadedPorts
      applyStates(running)
      const logLimit = useSettingsStore().logLimit
      await subscribeToServiceLogs((event) => {
        logs.value = [...logs.value.slice(-(logLimit - 1)), event]
      })
      await subscribeToServiceExit((serviceId) => {
        applyStates([{ serviceId, status: 'stopped' }])
      })
    } catch (cause) {
      reportError(cause)
    } finally {
      busy.value = false
    }
  }

  async function refreshPorts(): Promise<void> {
    try {
      ports.value = await workbench.commands.execute<undefined, PortInfo[]>('port.list', undefined, workbench.commandContext)
    } catch (cause) {
      reportError(cause)
    }
  }

  async function refreshProjects(): Promise<void> {
    try {
      projects.value = await workbench.projects.list()
    } catch (cause) {
      reportError(cause)
    }
  }

  async function refreshRunning(): Promise<void> {
    applyStates(await workbench.services.refreshRunning())
  }

  async function openProject(path: string): Promise<void> {
    busy.value = true
    error.value = undefined
    try {
      const context = await workbench.projects.open(path)
      activeContext.value = context
      activeProjectId.value = context.id
      logServiceFilter.value = ALL_SERVICES
      await Promise.all([refreshProjects(), loadServices()])
    } catch (cause) {
      reportError(cause)
    } finally {
      busy.value = false
    }
  }

  async function chooseProject(): Promise<void> {
    const path = await open({ directory: true, multiple: false, title: 'Open development project' })
    if (typeof path === 'string' && path) await openProject(path)
  }

  async function activateProject(project: Project): Promise<void> {
    await openProject(project.path)
  }

  async function removeProject(project: Project): Promise<boolean> {
    busy.value = true
    error.value = undefined
    try {
      const removed = await workbench.projects.remove(project.id)
      if (!removed) return false
      projects.value = projects.value.filter(({ id }) => id !== project.id)
      if (activeProjectId.value === project.id) {
        activeProjectId.value = undefined
        activeContext.value = undefined
        workbench.context.setActiveProject()
        services.value = []
        logs.value = []
        logServiceFilter.value = ALL_SERVICES
      }
      return true
    } catch (cause) {
      reportError(cause)
      return false
    } finally {
      busy.value = false
    }
  }

  async function revealProject(project: Project): Promise<void> {
    try {
      await workbench.projects.reveal(project.path)
    } catch (cause) {
      reportError(cause)
    }
  }

  /** Re-reads manifests so newly added scripts and services show up. */
  async function rescan(): Promise<void> {
    const project = activeProject.value
    if (!project) return
    scanning.value = true
    try {
      activeContext.value = await workbench.commands.execute<Project, ProjectContext>('project.refresh', project, workbench.commandContext)
      await loadServices()
    } catch (cause) {
      reportError(cause)
    } finally {
      scanning.value = false
    }
  }

  async function loadServices(): Promise<void> {
    const projectId = activeContext.value?.id
    if (!projectId) {
      services.value = []
      return
    }
    services.value = await workbench.catalog.list(projectId)
    await refreshRunning()
  }

  function newServiceDraft(): DevService {
    return {
      id: createServiceId(),
      projectId: activeContext.value?.id ?? '',
      name: '',
      command: '',
      args: [],
      env: {},
      autoOpen: false,
      dependencies: [],
      updatedAt: 0,
    }
  }

  function draftFromSuggestion(suggestion: SuggestedService): DevService {
    return serviceFromSuggestion(
      activeContext.value?.id ?? '',
      suggestion,
      services.value.map((service) => service.name),
    )
  }

  async function saveService(service: DevService): Promise<boolean> {
    error.value = undefined
    try {
      await workbench.catalog.save(service)
      await loadServices()
      return true
    } catch (cause) {
      reportError(cause)
      return false
    }
  }

  async function importSuggestion(suggestion: SuggestedService): Promise<boolean> {
    return saveService(draftFromSuggestion(suggestion))
  }

  async function deleteService(serviceId: string): Promise<void> {
    error.value = undefined
    try {
      await workbench.catalog.remove(serviceId)
      if (logServiceFilter.value === serviceId) logServiceFilter.value = ALL_SERVICES
      await loadServices()
    } catch (cause) {
      reportError(cause)
    }
  }

  async function startService(service: DevService): Promise<void> {
    error.value = undefined
    try {
      applyStates([await workbench.services.start(service)])
    } catch (cause) {
      reportError(cause)
      applyStates([workbench.services.getState(service.id)])
    }
  }

  async function stopService(serviceId: string): Promise<void> {
    error.value = undefined
    try {
      applyStates([await workbench.services.stop(serviceId)])
    } catch (cause) {
      reportError(cause)
      applyStates([workbench.services.getState(serviceId)])
    }
  }

  async function restartService(service: DevService): Promise<void> {
    error.value = undefined
    try {
      applyStates([await workbench.services.restart(service)])
    } catch (cause) {
      reportError(cause)
      applyStates([workbench.services.getState(service.id)])
    }
  }

  async function startAll(): Promise<BatchOutcome | undefined> {
    error.value = undefined
    try {
      const outcome = await workbench.services.startAll(services.value)
      applyStates(workbench.services.listStates())
      return outcome
    } catch (cause) {
      reportError(cause)
      applyStates(workbench.services.listStates())
      return undefined
    }
  }

  async function stopAll(): Promise<BatchOutcome | undefined> {
    error.value = undefined
    try {
      const outcome = await workbench.services.stopAll(services.value)
      applyStates(workbench.services.listStates())
      return outcome
    } catch (cause) {
      reportError(cause)
      applyStates(workbench.services.listStates())
      return undefined
    }
  }

  async function stopEverything(): Promise<void> {
    error.value = undefined
    try {
      await workbench.services.stopEverything()
      await refreshRunning()
    } catch (cause) {
      reportError(cause)
    }
  }

  function clearLogs(): void {
    logs.value = []
  }

  return {
    projects, ports, services, serviceStates, activeContext, activeProjectId,
    logs, logServiceFilter, error, busy, scanning,
    activeProject, visibleLogs, runningCount, suggestedServices,
    stateOf, initialize, refreshPorts, refreshProjects, refreshRunning,
    openProject, chooseProject, activateProject, removeProject, revealProject, rescan, loadServices,
    newServiceDraft, draftFromSuggestion, saveService, importSuggestion, deleteService,
    startService, stopService, restartService, startAll, stopAll, stopEverything,
    clearLogs, dismissError,
  }
})
