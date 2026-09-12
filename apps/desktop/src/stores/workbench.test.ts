import type { DevService, Project, ProjectContext } from '@dev-workbench/shared'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'

/** Fixtures and the fake native bridge must exist before the store imports run. */
const fixtures = vi.hoisted(() => {
  const project = {
    id: 'project-1',
    name: 'demo',
    path: 'C:/projects/demo',
    createdAt: 1,
    updatedAt: 2,
    lastOpenedAt: 2,
  } as unknown as import('@dev-workbench/shared').Project

  const context = {
    id: 'project-1',
    name: 'demo',
    path: 'C:/projects/demo',
    languages: ['Rust'],
    frameworks: ['Tauri'],
    packageManagers: ['Cargo'],
    detectedFiles: [],
    scripts: [],
    suggestedServices: [
      { name: 'demo:dev', command: 'pnpm', args: ['dev'], port: 5173, source: 'package.json' },
    ],
    composeServices: [],
    monorepo: false,
    scannedAt: 3,
  } as unknown as import('@dev-workbench/shared').ProjectContext

  const services = [
    {
      id: 'service-1',
      projectId: 'project-1',
      name: 'api',
      command: 'pnpm',
      args: ['dev'],
      env: {},
      autoOpen: false,
      dependencies: [],
      updatedAt: 0,
    },
  ] as unknown as import('@dev-workbench/shared').DevService[]

  return { project, context, services }
})

const bridge = vi.hoisted(() => ({
  listProjects: vi.fn(),
  openProject: vi.fn(),
  deleteProject: vi.fn(),
  revealProject: vi.fn(),
  scanProject: vi.fn(),
  listServices: vi.fn(),
  saveService: vi.fn(),
  deleteService: vi.fn(),
  startService: vi.fn(),
  stopService: vi.fn(),
  stopAllServices: vi.fn(),
  listRunningServices: vi.fn(),
  listProcesses: vi.fn(),
  processTree: vi.fn(),
  killProcess: vi.fn(),
  listPorts: vi.fn(),
  killPort: vi.fn(),
}))

vi.mock('../services/nativeBridge', () => ({
  nativeBridge: {},
  subscribeToServiceLogs: vi.fn(async () => () => {}),
  subscribeToServiceExit: vi.fn(async () => () => {}),
}))

vi.mock('@tauri-apps/plugin-dialog', () => ({
  open: vi.fn(async () => 'C:/projects/demo'),
}))

vi.mock('../services/workbench', async () => {
  const { createWorkbench } = await import('@dev-workbench/core')
  return { workbench: createWorkbench(bridge as never) }
})

import { subscribeToServiceLogs } from '../services/nativeBridge'
import { useWorkbenchStore } from './workbench'

function service(overrides: Partial<DevService> = {}): DevService {
  return { ...fixtures.services[0]!, ...overrides }
}

beforeEach(() => {
  setActivePinia(createPinia())
  vi.clearAllMocks()
  bridge.listProjects.mockResolvedValue([fixtures.project])
  bridge.openProject.mockResolvedValue(fixtures.project)
  bridge.deleteProject.mockResolvedValue(true)
  bridge.revealProject.mockResolvedValue(undefined)
  bridge.scanProject.mockResolvedValue(fixtures.context)
  bridge.listServices.mockResolvedValue([service()])
  bridge.listRunningServices.mockResolvedValue([])
  bridge.listPorts.mockResolvedValue([{ port: 5173, pid: 42, processName: 'node', address: '127.0.0.1:5173' }])
  bridge.saveService.mockResolvedValue(undefined)
  bridge.deleteService.mockResolvedValue(true)
  bridge.stopAllServices.mockResolvedValue(0)
  bridge.startService.mockImplementation(async (target: DevService) => ({ serviceId: target.id, status: 'running', pid: 99 }))
  bridge.stopService.mockImplementation(async (serviceId: string) => ({ serviceId, status: 'stopped' }))
})

describe('workbench store', () => {
  it('loads projects, ports and running services on initialize', async () => {
    const store = useWorkbenchStore()
    await store.initialize()
    expect(store.projects).toHaveLength(1)
    expect(store.ports).toHaveLength(1)
    expect(store.runningCount).toBe(0)
    expect(store.error).toBeUndefined()
    expect(subscribeToServiceLogs).toHaveBeenCalledOnce()
  })

  it('reflects services that the native runtime already runs', async () => {
    bridge.listRunningServices.mockResolvedValue([{ serviceId: 'service-1', pid: 7 }])
    const store = useWorkbenchStore()
    await store.initialize()
    expect(store.stateOf('service-1')).toEqual({ serviceId: 'service-1', status: 'running', pid: 7 })
  })

  it('opens the chosen project and loads its services and suggestions', async () => {
    const store = useWorkbenchStore()
    await store.chooseProject()
    expect(store.activeProjectId).toBe('project-1')
    expect(store.activeProject?.name).toBe('demo')
    expect(store.activeContext?.frameworks).toEqual(['Tauri'])
    expect(store.services.map((item) => item.id)).toEqual(['service-1'])
    expect(store.suggestedServices.map((item) => item.name)).toEqual(['demo:dev'])
  })

  it('activates a project from the recent list', async () => {
    const store = useWorkbenchStore()
    await store.initialize()
    await store.activateProject(fixtures.project)
    expect(bridge.openProject).toHaveBeenCalledWith(fixtures.project.path)
    expect(store.activeProjectId).toBe('project-1')
  })

  it('removes the active project record without touching the project path', async () => {
    const store = useWorkbenchStore()
    await store.chooseProject()
    await expect(store.removeProject(fixtures.project)).resolves.toBe(true)
    expect(bridge.deleteProject).toHaveBeenCalledWith('project-1')
    expect(store.projects).toEqual([])
    expect(store.activeProjectId).toBeUndefined()
    expect(store.activeContext).toBeUndefined()
  })

  it('reports a failed project open', async () => {
    bridge.openProject.mockRejectedValue({ code: 'VALIDATION_ERROR', message: 'project path must be an existing directory' })
    const store = useWorkbenchStore()
    await store.openProject('C:/missing')
    expect(store.error).toBe('project path must be an existing directory')
    expect(store.activeContext).toBeUndefined()
  })

  it('re-scans the active project', async () => {
    const store = useWorkbenchStore()
    await store.chooseProject()
    bridge.scanProject.mockResolvedValue({ ...fixtures.context, frameworks: ['Tauri', 'Vite'] })
    await store.rescan()
    expect(store.scanning).toBe(false)
    expect(store.activeContext?.frameworks).toEqual(['Tauri', 'Vite'])
  })

  it('tracks service start, restart and stop', async () => {
    const store = useWorkbenchStore()
    await store.chooseProject()
    const target = service()

    await store.startService(target)
    expect(store.stateOf(target.id).status).toBe('running')
    expect(store.runningCount).toBe(1)

    await store.restartService(target)
    expect(bridge.stopService).toHaveBeenCalledWith(target.id)
    expect(store.stateOf(target.id).status).toBe('running')

    await store.stopService(target.id)
    expect(store.stateOf(target.id).status).toBe('stopped')
    expect(store.runningCount).toBe(0)
  })

  it('surfaces a start failure and marks the service failed', async () => {
    bridge.startService.mockRejectedValue({ code: 'NATIVE_ERROR', message: 'port already in use' })
    const store = useWorkbenchStore()
    await store.chooseProject()
    await store.startService(service())
    expect(store.error).toBe('port already in use')
    expect(store.stateOf('service-1').status).toBe('failed')
    expect(store.stateOf('service-1').error?.message).toBe('port already in use')
  })

  it('saves a service and reloads the list from the backend', async () => {
    const store = useWorkbenchStore()
    await store.chooseProject()
    bridge.listServices.mockResolvedValue([service(), service({ id: 'service-2', name: 'web' })])

    await expect(store.saveService(service({ name: 'api' }))).resolves.toBe(true)
    expect(bridge.saveService).toHaveBeenCalledOnce()
    expect(store.services).toHaveLength(2)
  })

  it('refuses to save an invalid service', async () => {
    const store = useWorkbenchStore()
    await store.chooseProject()
    await expect(store.saveService(service({ command: '   ' }))).resolves.toBe(false)
    expect(bridge.saveService).not.toHaveBeenCalled()
    expect(store.error).toContain('commandRequired')
  })

  it('imports a scan suggestion as a new service', async () => {
    const store = useWorkbenchStore()
    await store.chooseProject()
    await store.importSuggestion(store.suggestedServices[0]!)
    expect(bridge.saveService).toHaveBeenCalledWith(expect.objectContaining({
      projectId: 'project-1',
      name: 'demo:dev',
      command: 'pnpm',
      args: ['dev'],
      port: 5173,
    }))
  })

  it('creates a blank draft bound to the active project', async () => {
    const store = useWorkbenchStore()
    await store.chooseProject()
    expect(store.newServiceDraft()).toMatchObject({ projectId: 'project-1', name: '', command: '' })
  })

  it('deletes a service and resets the log filter', async () => {
    const store = useWorkbenchStore()
    await store.chooseProject()
    store.logServiceFilter = 'service-1'
    bridge.listServices.mockResolvedValue([])

    await store.deleteService('service-1')
    expect(bridge.deleteService).toHaveBeenCalledWith('service-1')
    expect(store.services).toEqual([])
    expect(store.logServiceFilter).toBe('all')
  })

  it('starts every service and reports the ones that failed', async () => {
    const store = useWorkbenchStore()
    await store.chooseProject()
    bridge.listServices.mockResolvedValue([service({ id: 'db', name: 'db' }), service({ id: 'api', name: 'api', dependencies: ['db'] })])
    await store.loadServices()
    bridge.startService.mockImplementation(async (target: DevService) => {
      if (target.id === 'api') throw new Error('boom')
      return { serviceId: target.id, status: 'running', pid: 1 }
    })

    const outcome = await store.startAll()
    expect(outcome?.succeeded.map((state) => state.serviceId)).toEqual(['db'])
    expect(outcome?.failed.map((failure) => failure.serviceId)).toEqual(['api'])
  })

  it('reports a dependency cycle instead of starting anything', async () => {
    const store = useWorkbenchStore()
    await store.chooseProject()
    bridge.listServices.mockResolvedValue([
      service({ id: 'a', name: 'a', dependencies: ['b'] }),
      service({ id: 'b', name: 'b', dependencies: ['a'] }),
    ])
    await store.loadServices()
    await expect(store.startAll()).resolves.toBeUndefined()
    expect(store.error).toContain('cycle')
    expect(bridge.startService).not.toHaveBeenCalled()
  })

  it('filters and clears the log buffer', async () => {
    const store = useWorkbenchStore()
    await store.initialize()
    const handler = vi.mocked(subscribeToServiceLogs).mock.calls[0]?.[0]
    handler?.({ serviceId: 'service-1', stream: 'stdout', line: 'ready', timestamp: 1 })
    handler?.({ serviceId: 'service-2', stream: 'stderr', line: 'failed', timestamp: 2 })
    expect(store.logs).toHaveLength(2)

    store.logServiceFilter = 'service-1'
    expect(store.visibleLogs.map((log) => log.line)).toEqual(['ready'])

    store.clearLogs()
    expect(store.logs).toEqual([])
  })
})
