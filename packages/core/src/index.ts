import { CommandRegistry, type Command, type CommandContext, type ServiceContainer as ServiceContainerContract } from '@dev-workbench/command'
import { ContextStore } from '@dev-workbench/context'
import { AppError, createServiceId, validateDevService } from '@dev-workbench/shared'
import type { ApiModule, DevService, HttpRequest, HttpResponse, PortInfo, ProcessInfo, Project, ProjectContext, RunningService, SavedApiRequest, ServiceState, SuggestedService } from '@dev-workbench/shared'
import { SettingsService } from './settings'
import { SystemService } from './system'

export * from './settings'
export * from './system'

export interface NativeBridge {
  listProjects(): Promise<Project[]>
  openProject(path: string): Promise<Project>
  deleteProject(projectId: string): Promise<boolean>
  revealProject(path: string): Promise<void>
  scanProject(project: Project): Promise<ProjectContext>
  listServices(projectId: string): Promise<DevService[]>
  saveService(service: DevService): Promise<void>
  deleteService(serviceId: string): Promise<boolean>
  startService(service: DevService): Promise<ServiceState>
  stopService(serviceId: string): Promise<ServiceState>
  stopAllServices(): Promise<number>
  listRunningServices(): Promise<RunningService[]>
  listProcesses(): Promise<ProcessInfo[]>
  processTree(pid: number): Promise<ProcessInfo[]>
  killProcess(pid: number): Promise<void>
  listPorts(): Promise<PortInfo[]>
  killPort(port: number): Promise<void>
  getSettings(): Promise<Record<string, unknown>>
  setSetting(key: string, value: unknown): Promise<void>
  httpRequest(request: HttpRequest): Promise<HttpResponse>
  listApiModules(): Promise<ApiModule[]>
  saveApiModule(module: ApiModule): Promise<ApiModule>
  deleteApiModule(moduleId: string): Promise<boolean>
  listApiRequests(moduleId?: string): Promise<SavedApiRequest[]>
  saveApiRequest(request: SavedApiRequest): Promise<SavedApiRequest>
  deleteApiRequest(requestId: string): Promise<boolean>
}

export class ServiceContainer implements ServiceContainerContract {
  private readonly values = new Map<string, unknown>()

  set<T>(key: string, value: T): this {
    this.values.set(key, value)
    return this
  }

  get<T>(key: string): T {
    if (!this.values.has(key)) throw new Error(`Service is not registered: ${key}`)
    return this.values.get(key) as T
  }
}

export class ProjectService {
  constructor(private readonly bridge: NativeBridge, private readonly context: ContextStore) {}
  list(): Promise<Project[]> { return this.bridge.listProjects() }
  remove(projectId: string): Promise<boolean> { return this.bridge.deleteProject(projectId) }
  reveal(path: string): Promise<void> { return this.bridge.revealProject(path) }
  async open(path: string): Promise<ProjectContext> {
    const project = await this.bridge.openProject(path)
    const projectContext = await this.bridge.scanProject(project)
    this.context.setProject(projectContext)
    this.context.setActiveProject(project.id)
    return projectContext
  }
  async refresh(project: Project): Promise<ProjectContext> {
    const projectContext = await this.bridge.scanProject(project)
    this.context.setProject(projectContext)
    return projectContext
  }
}

/** Turns a scanner suggestion into a persistable service draft. */
export function serviceFromSuggestion(
  projectId: string,
  suggestion: SuggestedService,
  takenNames: Iterable<string> = [],
): DevService {
  const taken = new Set(takenNames)
  const base = suggestion.name.trim() || 'service'
  let name = base
  let suffix = 2
  while (taken.has(name)) {
    name = `${base}-${suffix}`
    suffix += 1
  }
  return {
    id: createServiceId(),
    projectId,
    name,
    command: suggestion.command,
    args: [...suggestion.args],
    env: {},
    autoOpen: false,
    dependencies: [],
    updatedAt: 0,
    ...(suggestion.cwd ? { cwd: suggestion.cwd } : {}),
    ...(suggestion.port === undefined ? {} : { port: suggestion.port }),
  }
}

/** Trims and de-duplicates user input before it crosses the native boundary. */
export function normalizeService(service: DevService): DevService {
  const cwd = service.cwd?.trim()
  const port = service.port
  return {
    id: service.id.trim(),
    projectId: service.projectId.trim(),
    name: service.name.trim(),
    command: service.command.trim(),
    args: (service.args ?? []).map((arg) => arg.trim()).filter((arg) => arg.length > 0),
    env: Object.fromEntries(
      Object.entries(service.env ?? {})
        .map(([key, value]) => [key.trim(), value] as const)
        .filter(([key]) => key.length > 0),
    ),
    autoOpen: service.autoOpen ?? false,
    dependencies: [...new Set((service.dependencies ?? []).filter((id) => id.length > 0))],
    updatedAt: service.updatedAt ?? 0,
    ...(cwd ? { cwd } : {}),
    ...(port === undefined ? {} : { port }),
  }
}

/** Persists service definitions for a project. */
export class ServiceCatalog {
  constructor(private readonly bridge: NativeBridge) {}

  list(projectId: string): Promise<DevService[]> {
    return this.bridge.listServices(projectId)
  }

  async save(service: DevService): Promise<DevService> {
    const normalized = normalizeService(service)
    const issues = validateDevService(normalized)
    if (issues.length) {
      throw new AppError('VALIDATION_ERROR', `Service is invalid: ${issues.join(', ')}`, { issues })
    }
    await this.bridge.saveService(normalized)
    return normalized
  }

  remove(serviceId: string): Promise<boolean> {
    return this.bridge.deleteService(serviceId)
  }
}

/** Application service for persisted API modules and request templates. */
export class ApiCatalog {
  constructor(private readonly bridge: NativeBridge) {}

  listModules(): Promise<ApiModule[]> { return this.bridge.listApiModules() }
  saveModule(module: ApiModule): Promise<ApiModule> { return this.bridge.saveApiModule(module) }
  removeModule(moduleId: string): Promise<boolean> { return this.bridge.deleteApiModule(moduleId) }
  listRequests(moduleId?: string): Promise<SavedApiRequest[]> { return this.bridge.listApiRequests(moduleId) }
  saveRequest(request: SavedApiRequest): Promise<SavedApiRequest> { return this.bridge.saveApiRequest(request) }
  removeRequest(requestId: string): Promise<boolean> { return this.bridge.deleteApiRequest(requestId) }
}

export interface BatchOutcome {
  succeeded: ServiceState[]
  failed: Array<{ serviceId: string; error: AppError }>
}

/** Resolves dependency order for start-up; dependents always come last. */
export function orderByDependencies(services: DevService[]): DevService[] {
  const pending = new Map(services.map((service) => [service.id, service]))
  const resolved = new Set<string>()
  const ordered: DevService[] = []
  while (pending.size) {
    const ready = [...pending.values()].filter((service) =>
      (service.dependencies ?? []).every((id) => resolved.has(id) || !pending.has(id)))
    if (!ready.length) {
      throw new AppError('VALIDATION_ERROR', 'Service dependency cycle detected', {
        serviceIds: [...pending.keys()],
      })
    }
    for (const service of ready) {
      ordered.push(service)
      pending.delete(service.id)
      resolved.add(service.id)
    }
  }
  return ordered
}

export class ServiceManager {
  private readonly states = new Map<string, ServiceState>()
  constructor(private readonly bridge: NativeBridge) {}

  getState(serviceId: string): ServiceState {
    return this.states.get(serviceId) ?? { serviceId, status: 'stopped' }
  }

  listStates(): ServiceState[] {
    return [...this.states.values()]
  }

  /** Pulls the authoritative running set from the native runtime. */
  async refreshRunning(): Promise<ServiceState[]> {
    return this.syncRunning(await this.bridge.listRunningServices())
  }

  /** Reconciles local state with the services the native runtime still owns. */
  syncRunning(running: RunningService[]): ServiceState[] {
    const alive = new Map(running.map((item) => [item.serviceId, item]))
    for (const [serviceId, state] of this.states) {
      if (state.status !== 'stopped' && !alive.has(serviceId)) {
        this.states.set(serviceId, { serviceId, status: 'stopped' })
      }
    }
    for (const item of running) {
      this.states.set(item.serviceId, { serviceId: item.serviceId, status: 'running', pid: item.pid })
    }
    return this.listStates()
  }

  async start(service: DevService): Promise<ServiceState> {
    this.states.set(service.id, { serviceId: service.id, status: 'starting' })
    try {
      const state = await this.bridge.startService(service)
      this.states.set(service.id, state)
      return state
    } catch (error) {
      const appError = AppError.from(error)
      this.states.set(service.id, { serviceId: service.id, status: 'failed', error: { code: appError.code, message: appError.message } })
      throw appError
    }
  }

  async stop(serviceId: string): Promise<ServiceState> {
    this.states.set(serviceId, { serviceId, status: 'stopping' })
    try {
      const state = await this.bridge.stopService(serviceId)
      this.states.set(serviceId, state)
      return state
    } catch (error) {
      const appError = AppError.from(error)
      this.states.set(serviceId, { serviceId, status: 'failed', error: { code: appError.code, message: appError.message } })
      throw appError
    }
  }

  async restart(service: DevService): Promise<ServiceState> {
    await this.stop(service.id)
    return this.start(service)
  }

  /**
   * Starts services in dependency order, skipping any service whose
   * dependency failed instead of failing it for the same reason twice.
   */
  async startAll(services: DevService[]): Promise<BatchOutcome> {
    const outcome: BatchOutcome = { succeeded: [], failed: [] }
    const unavailable = new Set<string>()
    for (const service of orderByDependencies(services)) {
      const blockedBy = (service.dependencies ?? []).filter((id) => unavailable.has(id))
      if (blockedBy.length) {
        unavailable.add(service.id)
        outcome.failed.push({
          serviceId: service.id,
          error: new AppError('NATIVE_ERROR', `Skipped because a dependency failed: ${blockedBy.join(', ')}`, { serviceIds: blockedBy }),
        })
        continue
      }
      try {
        outcome.succeeded.push(await this.start(service))
      } catch (error) {
        unavailable.add(service.id)
        outcome.failed.push({ serviceId: service.id, error: AppError.from(error) })
      }
    }
    return outcome
  }

  /** Stops dependents before the services they depend on. */
  async stopAll(services: DevService[]): Promise<BatchOutcome> {
    return this.runBatch(orderByDependencies(services).reverse(), (service) => this.stop(service.id))
  }

  /** Asks the native runtime to drop every process it owns. */
  async stopEverything(): Promise<number> {
    const stopped = await this.bridge.stopAllServices()
    for (const [serviceId, state] of this.states) {
      if (state.status !== 'stopped') this.states.set(serviceId, { serviceId, status: 'stopped' })
    }
    return stopped
  }

  private async runBatch(services: DevService[], run: (service: DevService) => Promise<ServiceState>): Promise<BatchOutcome> {
    const outcome: BatchOutcome = { succeeded: [], failed: [] }
    for (const service of services) {
      try {
        outcome.succeeded.push(await run(service))
      } catch (error) {
        outcome.failed.push({ serviceId: service.id, error: AppError.from(error) })
      }
    }
    return outcome
  }
}

export interface WorkbenchApplication {
  commands: CommandRegistry
  commandContext: CommandContext
  context: ContextStore
  projects: ProjectService
  services: ServiceManager
  catalog: ServiceCatalog
  api: ApiCatalog
  system: SystemService
  settings: SettingsService
}

export function createWorkbench(bridge: NativeBridge): WorkbenchApplication {
  const context = new ContextStore()
  const container = new ServiceContainer()
  const projects = new ProjectService(bridge, context)
  const services = new ServiceManager(bridge)
  const catalog = new ServiceCatalog(bridge)
  const api = new ApiCatalog(bridge)
  const system = new SystemService(bridge)
  const settings = new SettingsService(bridge)
  container
    .set('nativeBridge', bridge)
    .set('projects', projects)
    .set('services', services)
    .set('catalog', catalog)
    .set('api', api)
    .set('system', system)
    .set('settings', settings)
  const commands = new CommandRegistry()
  const commandContext: CommandContext = { workbench: { activeProjectId: undefined }, services: container }
  registerCoreCommands(commands)
  context.subscribe((state) => { commandContext.workbench.activeProjectId = state.activeProjectId })
  return { commands, commandContext, context, projects, services, catalog, api, system, settings }
}

function registerCoreCommands(registry: CommandRegistry): void {
  const definitions: Command[] = [
    { id: 'project.open', title: 'Open Project', category: 'Project', execute: (input, ctx) => ctx.services.get<ProjectService>('projects').open(input as string) },
    { id: 'project.remove', title: 'Remove Project', category: 'Project', execute: (input, ctx) => ctx.services.get<ProjectService>('projects').remove(input as string) },
    { id: 'project.reveal', title: 'Reveal Project', category: 'Project', execute: (input, ctx) => ctx.services.get<ProjectService>('projects').reveal(input as string) },
    { id: 'project.scan', title: 'Scan Project', category: 'Project', execute: (input, ctx) => ctx.services.get<NativeBridge>('nativeBridge').scanProject(input as Project) },
    { id: 'project.refresh', title: 'Refresh Project', category: 'Project', execute: (input, ctx) => ctx.services.get<ProjectService>('projects').refresh(input as Project) },
    { id: 'service.list', title: 'List Services', category: 'Services', execute: (input, ctx) => ctx.services.get<ServiceCatalog>('catalog').list(input as string) },
    { id: 'service.save', title: 'Save Service', category: 'Services', execute: (input, ctx) => ctx.services.get<ServiceCatalog>('catalog').save(input as DevService) },
    { id: 'service.delete', title: 'Delete Service', category: 'Services', execute: (input, ctx) => ctx.services.get<ServiceCatalog>('catalog').remove(input as string) },
    { id: 'service.start', title: 'Start Service', category: 'Services', execute: (input, ctx) => ctx.services.get<ServiceManager>('services').start(input as DevService) },
    { id: 'service.stop', title: 'Stop Service', category: 'Services', execute: (input, ctx) => ctx.services.get<ServiceManager>('services').stop(input as string) },
    { id: 'service.restart', title: 'Restart Service', category: 'Services', execute: (input, ctx) => ctx.services.get<ServiceManager>('services').restart(input as DevService) },
    { id: 'service.startAll', title: 'Start All Services', category: 'Services', execute: (input, ctx) => ctx.services.get<ServiceManager>('services').startAll(input as DevService[]) },
    { id: 'service.stopAll', title: 'Stop All Services', category: 'Services', execute: (input, ctx) => ctx.services.get<ServiceManager>('services').stopAll(input as DevService[]) },
    { id: 'service.stopEverything', title: 'Stop Every Running Service', category: 'Services', execute: (_, ctx) => ctx.services.get<ServiceManager>('services').stopEverything() },
    { id: 'service.listRunning', title: 'List Running Services', category: 'Services', execute: (_, ctx) => ctx.services.get<ServiceManager>('services').refreshRunning() },
    { id: 'process.list', title: 'List Processes', category: 'System', execute: (_, ctx) => ctx.services.get<SystemService>('system').listProcesses() },
    { id: 'process.tree', title: 'Show Process Tree', category: 'System', execute: (input, ctx) => ctx.services.get<SystemService>('system').processTree(input as number) },
    { id: 'process.kill', title: 'Kill Process Tree', category: 'System', execute: (input, ctx) => ctx.services.get<SystemService>('system').killProcess(input as number) },
    { id: 'port.list', title: 'List Listening Ports', category: 'System', execute: (_, ctx) => ctx.services.get<SystemService>('system').listPorts() },
    { id: 'port.find', title: 'Find Listening Port', category: 'System', execute: async (input, ctx) => (await ctx.services.get<SystemService>('system').listPorts()).find(({ port }) => port === input) },
    { id: 'port.kill', title: 'Kill Process Tree on Port', category: 'System', execute: (input, ctx) => ctx.services.get<SystemService>('system').killPort(input as number) },
    { id: 'settings.load', title: 'Load Settings', category: 'Settings', execute: (_, ctx) => ctx.services.get<SettingsService>('settings').load() },
    { id: 'settings.save', title: 'Save Setting', category: 'Settings', execute: (input, ctx) => ctx.services.get<SettingsService>('settings').saveAll(input as Record<string, unknown>) },
    { id: 'utility.sql.open', title: 'Open SQL Workbench', category: 'Utilities / Database', execute: async () => 'sql' },
    { id: 'utility.mybatis.restore.open', title: 'Open MyBatis SQL Restore', category: 'Utilities / Database', execute: async () => 'mybatis-restore' },
    { id: 'utility.api.open', title: 'Open API Workbench', category: 'Utilities / API', execute: async () => 'api' },
    { id: 'utility.api.send', title: 'Send API Request', category: 'Utilities / API', execute: (input, ctx) => ctx.services.get<NativeBridge>('nativeBridge').httpRequest(input as HttpRequest) },
    { id: 'api.module.list', title: 'List API Modules', category: 'API', execute: (_, ctx) => ctx.services.get<ApiCatalog>('api').listModules() },
    { id: 'api.module.save', title: 'Save API Module', category: 'API', execute: (input, ctx) => ctx.services.get<ApiCatalog>('api').saveModule(input as ApiModule) },
    { id: 'api.module.delete', title: 'Delete API Module', category: 'API', execute: (input, ctx) => ctx.services.get<ApiCatalog>('api').removeModule(input as string) },
    { id: 'api.request.list', title: 'List Saved API Requests', category: 'API', execute: (input, ctx) => ctx.services.get<ApiCatalog>('api').listRequests(input as string | undefined) },
    { id: 'api.request.save', title: 'Save API Request', category: 'API', execute: (input, ctx) => ctx.services.get<ApiCatalog>('api').saveRequest(input as SavedApiRequest) },
    { id: 'api.request.delete', title: 'Delete Saved API Request', category: 'API', execute: (input, ctx) => ctx.services.get<ApiCatalog>('api').removeRequest(input as string) },
  ]
  definitions.forEach((command) => registry.register(command))
}
