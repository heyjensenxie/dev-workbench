import { ContextStore } from '@dev-workbench/context'
import { AppError } from '@dev-workbench/shared'
import type { DevService, Project, ProjectContext, ServiceState, SuggestedService } from '@dev-workbench/shared'
import { describe, expect, it, vi } from 'vitest'
import {
  createWorkbench,
  normalizeService,
  orderByDependencies,
  ProjectService,
  serviceFromSuggestion,
  ServiceCatalog,
  ServiceManager,
  SettingsService,
  SystemService,
  type NativeBridge,
} from './index'

const PROJECT: Project = {
  id: 'project-1',
  name: 'demo',
  path: '/projects/demo',
  createdAt: 1,
  updatedAt: 2,
  lastOpenedAt: 2,
}

const PROJECT_CONTEXT: ProjectContext = {
  id: 'project-1',
  name: 'demo',
  path: '/projects/demo',
  languages: ['Rust'],
  frameworks: ['Tauri'],
  packageManagers: ['Cargo'],
  detectedFiles: [],
  scripts: [],
  suggestedServices: [],
  composeServices: [],
  monorepo: false,
  scannedAt: 2,
}

function createFakeBridge(overrides: Partial<NativeBridge> = {}): NativeBridge {
  const bridge = {
    listProjects: vi.fn(async () => [PROJECT]),
    openProject: vi.fn(async () => PROJECT),
    deleteProject: vi.fn(async () => true),
    revealProject: vi.fn(async () => undefined),
    scanProject: vi.fn(async () => PROJECT_CONTEXT),
    listServices: vi.fn(async () => [] as DevService[]),
    saveService: vi.fn(async () => undefined),
    deleteService: vi.fn(async () => true),
    startService: vi.fn(async (service: DevService): Promise<ServiceState> => ({ serviceId: service.id, status: 'running', pid: 4242 })),
    stopService: vi.fn(async (serviceId: string): Promise<ServiceState> => ({ serviceId, status: 'stopped' })),
    stopAllServices: vi.fn(async () => 0),
    listRunningServices: vi.fn(async () => []),
    listProcesses: vi.fn(async () => []),
    processTree: vi.fn(async () => []),
    killProcess: vi.fn(async () => undefined),
    listPorts: vi.fn(async () => []),
    killPort: vi.fn(async () => undefined),
    getSettings: vi.fn(async () => ({}) as Record<string, unknown>),
    setSetting: vi.fn(async () => undefined),
    ...overrides,
  }
  return bridge as unknown as NativeBridge
}

function service(id: string, dependencies: string[] = []): DevService {
  return {
    id,
    projectId: 'project-1',
    name: id,
    command: 'pnpm',
    args: ['dev'],
    env: {},
    autoOpen: false,
    dependencies,
    updatedAt: 0,
  }
}

describe('orderByDependencies', () => {
  it('resolves dependencies before their dependents', () => {
    const ordered = orderByDependencies([
      service('web', ['api']),
      service('api', ['db']),
      service('db'),
    ])
    expect(ordered.map((item) => item.id)).toEqual(['db', 'api', 'web'])
  })

  it('treats unknown dependency ids as already satisfied', () => {
    expect(orderByDependencies([service('web', ['missing'])]).map((item) => item.id)).toEqual(['web'])
  })

  it('rejects dependency cycles', () => {
    expect(() => orderByDependencies([service('a', ['b']), service('b', ['a'])])).toThrow(AppError)
  })
})

describe('ServiceManager', () => {
  it('reports a stopped service by default', () => {
    const manager = new ServiceManager(createFakeBridge())
    expect(manager.getState('service-1')).toEqual({ serviceId: 'service-1', status: 'stopped' })
  })

  it('tracks running state with its pid', async () => {
    const manager = new ServiceManager(createFakeBridge())
    await expect(manager.start(service('api'))).resolves.toEqual({ serviceId: 'api', status: 'running', pid: 4242 })
    expect(manager.getState('api').status).toBe('running')

    await manager.stop('api')
    expect(manager.getState('api')).toEqual({ serviceId: 'api', status: 'stopped' })
  })

  it('records a failure and rethrows', async () => {
    const bridge = createFakeBridge({
      startService: vi.fn(async () => {
        throw new AppError('NATIVE_ERROR', 'port already in use')
      }),
    })
    const manager = new ServiceManager(bridge)
    await expect(manager.start(service('api'))).rejects.toThrow('port already in use')
    expect(manager.getState('api').status).toBe('failed')
    expect(manager.getState('api').error?.message).toBe('port already in use')
  })

  it('starts every service in dependency order and collects failures', async () => {
    const started: string[] = []
    const bridge = createFakeBridge({
      startService: vi.fn(async (service: DevService): Promise<ServiceState> => {
        if (service.id === 'api') throw new AppError('NATIVE_ERROR', 'port already in use')
        started.push(service.id)
        return { serviceId: service.id, status: 'running', pid: 1 }
      }),
    })
    const manager = new ServiceManager(bridge)
    const outcome = await manager.startAll([service('web', ['api']), service('api', ['db']), service('db')])

    expect(started).toEqual(['db'])
    expect(bridge.startService).toHaveBeenCalledTimes(2)
    expect(outcome.succeeded.map((state) => state.serviceId)).toEqual(['db'])
    expect(outcome.failed.map((failure) => failure.serviceId)).toEqual(['api', 'web'])
    // A dependent of a failed service is reported, not started.
    expect(outcome.failed[1]?.error.message).toContain('api')
    expect(manager.getState('web').status).toBe('stopped')
  })

  it('stops dependents before their dependencies', async () => {
    const stopped: string[] = []
    const bridge = createFakeBridge({
      stopService: vi.fn(async (serviceId: string): Promise<ServiceState> => {
        stopped.push(serviceId)
        return { serviceId, status: 'stopped' }
      }),
    })
    const manager = new ServiceManager(bridge)
    await manager.stopAll([service('web', ['api']), service('api', ['db']), service('db')])
    expect(stopped).toEqual(['web', 'api', 'db'])
  })

  it('reconciles local state with the native runtime', () => {
    const manager = new ServiceManager(createFakeBridge())
    manager.syncRunning([{ serviceId: 'api', pid: 77 }])
    expect(manager.getState('api')).toEqual({ serviceId: 'api', status: 'running', pid: 77 })
    manager.syncRunning([])
    expect(manager.getState('api').status).toBe('stopped')
  })

  it('pulls the running set from the bridge', async () => {
    const bridge = createFakeBridge({ listRunningServices: vi.fn(async () => [{ serviceId: 'api', pid: 9 }]) })
    const manager = new ServiceManager(bridge)
    await expect(manager.refreshRunning()).resolves.toEqual([{ serviceId: 'api', status: 'running', pid: 9 }])
  })

  it('stops everything through the native runtime', async () => {
    const bridge = createFakeBridge({ stopAllServices: vi.fn(async () => 3) })
    const manager = new ServiceManager(bridge)
    await manager.start(service('api'))
    await expect(manager.stopEverything()).resolves.toBe(3)
    expect(manager.getState('api').status).toBe('stopped')
  })
})

describe('normalizeService', () => {
  it('trims, de-duplicates and drops empty entries', () => {
    expect(normalizeService({
      ...service('api'),
      name: '  api  ',
      command: ' pnpm ',
      args: ['dev', ' ', '--port', '5173'],
      env: { ' NODE_ENV ': 'development', '   ': 'dropped' },
      dependencies: ['db', 'db', ''],
    })).toMatchObject({
      name: 'api',
      command: 'pnpm',
      args: ['dev', '--port', '5173'],
      env: { NODE_ENV: 'development' },
      dependencies: ['db'],
    })
  })

  it('omits absent optional fields', () => {
    const normalized = normalizeService({ ...service('api'), cwd: '   ' })
    expect('cwd' in normalized).toBe(false)
    expect('port' in normalized).toBe(false)
  })
})

describe('ServiceCatalog', () => {
  it('normalizes before it persists', async () => {
    const bridge = createFakeBridge()
    const saved = await new ServiceCatalog(bridge).save({ ...service('api'), name: ' api ', command: ' pnpm ' })
    expect(saved.name).toBe('api')
    expect(bridge.saveService).toHaveBeenCalledWith(expect.objectContaining({ name: 'api', command: 'pnpm' }))
  })

  it('refuses to persist an invalid service', async () => {
    const bridge = createFakeBridge()
    await expect(new ServiceCatalog(bridge).save({ ...service('api'), command: '  ' }))
      .rejects.toMatchObject({ code: 'VALIDATION_ERROR' })
    expect(bridge.saveService).not.toHaveBeenCalled()
  })

  it('delegates deletion', async () => {
    const bridge = createFakeBridge()
    await expect(new ServiceCatalog(bridge).remove('api')).resolves.toBe(true)
    expect(bridge.deleteService).toHaveBeenCalledWith('api')
  })
})

describe('serviceFromSuggestion', () => {
  const suggestion: SuggestedService = {
    name: 'web:dev',
    command: 'pnpm',
    args: ['dev'],
    cwd: 'apps/web',
    port: 5173,
    source: 'package.json',
  }

  it('creates a persistable draft', () => {
    const draft = serviceFromSuggestion('project-1', suggestion)
    expect(draft).toMatchObject({
      projectId: 'project-1',
      name: 'web:dev',
      command: 'pnpm',
      args: ['dev'],
      cwd: 'apps/web',
      port: 5173,
      autoOpen: false,
      dependencies: [],
    })
    expect(draft.id).toBeTruthy()
  })

  it('avoids colliding with existing names', () => {
    const draft = serviceFromSuggestion('project-1', suggestion, ['web:dev', 'web:dev-2'])
    expect(draft.name).toBe('web:dev-3')
  })

  it('omits paths and ports that were not suggested', () => {
    const draft = serviceFromSuggestion('project-1', { name: 'api', command: 'go', args: ['run'], source: 'Makefile' })
    expect('cwd' in draft).toBe(false)
    expect('port' in draft).toBe(false)
  })
})

describe('ProjectService', () => {
  it('scans the project and marks it active', async () => {
    const context = new ContextStore()
    const bridge = createFakeBridge()
    const projects = new ProjectService(bridge, context)

    await expect(projects.open('/projects/demo')).resolves.toMatchObject({ id: 'project-1' })
    expect(bridge.openProject).toHaveBeenCalledWith('/projects/demo')
    expect(context.snapshot.activeProjectId).toBe('project-1')
    expect(context.snapshot.projects['project-1']?.frameworks).toEqual(['Tauri'])
  })

  it('refreshes an already known project', async () => {
    const context = new ContextStore()
    const projects = new ProjectService(createFakeBridge(), context)
    await expect(projects.refresh(PROJECT)).resolves.toMatchObject({ id: 'project-1' })
    expect(context.snapshot.projects['project-1']).toBeDefined()
  })

  it('delegates project removal and reveal actions to the native bridge', async () => {
    const bridge = createFakeBridge()
    const projects = new ProjectService(bridge, new ContextStore())
    await expect(projects.remove('project-1')).resolves.toBe(true)
    await expect(projects.reveal(PROJECT.path)).resolves.toBeUndefined()
    expect(bridge.deleteProject).toHaveBeenCalledWith('project-1')
    expect(bridge.revealProject).toHaveBeenCalledWith(PROJECT.path)
  })
})

describe('createWorkbench', () => {
  it('wires the application services and core commands', async () => {
    const app = createWorkbench(createFakeBridge())
    const ids = app.commands.list().map((command) => command.id)
    for (const id of ['project.open', 'project.remove', 'project.reveal', 'service.save', 'service.delete', 'service.startAll', 'service.stopAll', 'process.kill', 'port.kill', 'settings.load', 'settings.save']) {
      expect(ids).toContain(id)
    }
    expect(app.commandContext.services.get<ServiceCatalog>('catalog')).toBe(app.catalog)
    expect(app.commandContext.services.get<SystemService>('system')).toBe(app.system)
    expect(app.commandContext.services.get<SettingsService>('settings')).toBe(app.settings)
    await expect(app.commands.execute('port.list', undefined, app.commandContext)).resolves.toEqual([])
    await expect(app.commands.execute('settings.load', undefined, app.commandContext)).resolves.toEqual({})
  })

  it('keeps the command context in sync with the active project', () => {
    const app = createWorkbench(createFakeBridge())
    app.context.setActiveProject('project-1')
    expect(app.commandContext.workbench.activeProjectId).toBe('project-1')
  })
})
