import type { PortInfo, ProcessInfo } from '@dev-workbench/shared'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const fixtures = vi.hoisted(() => ({
  processes: [
    { pid: 1, name: 'explorer', memoryBytes: 100, command: 'explorer.exe' },
    { pid: 2, name: 'node', memoryBytes: 900, parentPid: 1, command: 'node vite.js' },
    { pid: 3, name: 'chrome', memoryBytes: 500, command: 'chrome.exe', executable: 'C:\\chrome.exe' },
  ] as ProcessInfo[],
  ports: [
    { port: 5173, pid: 2, processName: 'node', address: '127.0.0.1:5173' },
    { port: 8080, pid: 3, processName: 'chrome', address: '0.0.0.0:8080' },
  ] as PortInfo[],
}))

const bridge = vi.hoisted(() => ({
  listProjects: vi.fn(),
  openProject: vi.fn(),
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
  getSettings: vi.fn(),
  setSetting: vi.fn(),
}))

vi.mock('../services/workbench', async () => {
  const { createWorkbench } = await import('@dev-workbench/core')
  return { workbench: createWorkbench(bridge as never) }
})

import { useSystemStore } from './system'

beforeEach(() => {
  setActivePinia(createPinia())
  vi.clearAllMocks()
  bridge.listProcesses.mockResolvedValue(fixtures.processes.map((process) => ({ ...process })))
  bridge.listPorts.mockResolvedValue(fixtures.ports.map((port) => ({ ...port })))
  bridge.killProcess.mockResolvedValue(undefined)
  bridge.killPort.mockResolvedValue(undefined)
})

describe('system store', () => {
  it('loads processes and ports', async () => {
    const store = useSystemStore()
    await store.refresh()
    expect(store.processes).toHaveLength(3)
    expect(store.ports).toHaveLength(2)
    expect(store.loadingProcesses).toBe(false)
    expect(store.loadingPorts).toBe(false)
    expect(store.error).toBeUndefined()
  })

  it('sorts processes by memory by default', async () => {
    const store = useSystemStore()
    await store.refreshProcesses()
    expect(store.visibleProcesses.map((process) => process.pid)).toEqual([2, 3, 1])
    expect(store.processesMemory).toBe('1.5 KB')
  })

  it('orders processes by tree on request', async () => {
    const store = useSystemStore()
    await store.refreshProcesses()
    store.processOrder = 'tree'
    // Roots stay memory-ordered (chrome 500 B before explorer 100 B) and every
    // child follows its parent, so node lands directly after explorer.
    expect(store.visibleProcesses.map((process) => process.pid)).toEqual([3, 1, 2])
    expect(store.processDepths.get(2)).toBe(1)
    expect(store.processDepths.get(3)).toBe(0)
  })

  it('keeps explicit sorting separate from process tree mode', async () => {
    const store = useSystemStore()
    await store.refreshProcesses()
    store.processSort = 'name'
    expect(store.visibleProcesses.map((process) => process.pid)).toEqual([3, 1, 2])

    store.setProcessTree(true)
    expect(store.processTree).toBe(true)
    expect(store.processOrder).toBe('tree')
    expect(store.processSort).toBe('name')

    store.setProcessTree(false)
    store.processSort = 'pid'
    expect(store.visibleProcesses.map((process) => process.pid)).toEqual([1, 2, 3])
  })

  it('filters processes and ports by query', async () => {
    const store = useSystemStore()
    await store.refresh()
    store.processQuery = 'chrome'
    expect(store.visibleProcesses.map((process) => process.pid)).toEqual([3])
    store.processQuery = 'vite.js'
    expect(store.visibleProcesses.map((process) => process.pid)).toEqual([2])
    store.processQuery = ''

    store.portQuery = '5173'
    expect(store.filteredPorts.map((port) => port.port)).toEqual([5173])
    store.portQuery = 'chrome'
    expect(store.filteredPorts.map((port) => port.port)).toEqual([8080])
  })

  it('kills a process immediately when confirmation is disabled', async () => {
    const store = useSystemStore()
    await store.refreshProcesses()
    await store.requestKillProcess(2, false)
    expect(bridge.killProcess).toHaveBeenCalledWith(2)
    expect(store.pendingKill).toBeUndefined()
    // The list is reloaded after the kill.
    expect(bridge.listProcesses).toHaveBeenCalledTimes(2)
  })

  it('arms and then confirms a kill when confirmation is enabled', async () => {
    const store = useSystemStore()
    await store.refreshProcesses()
    await store.requestKillProcess(3, true)
    expect(bridge.killProcess).not.toHaveBeenCalled()
    expect(store.pendingKill).toBe('process:3')

    await store.requestKillProcess(3, true)
    expect(bridge.killProcess).toHaveBeenCalledWith(3)
    expect(store.pendingKill).toBeUndefined()
  })

  it('cancels an armed kill', async () => {
    const store = useSystemStore()
    await store.requestKillProcess(3, true)
    store.cancelPendingKill()
    expect(store.pendingKill).toBeUndefined()
    expect(bridge.killProcess).not.toHaveBeenCalled()
  })

  it('frees a port and refreshes both lists', async () => {
    const store = useSystemStore()
    await store.refresh()
    await store.requestFreePort(5173, false)
    expect(bridge.killPort).toHaveBeenCalledWith(5173)
    expect(bridge.listPorts).toHaveBeenCalledTimes(2)
    expect(bridge.listProcesses).toHaveBeenCalledTimes(2)
  })

  it('reports a failed kill without losing the list', async () => {
    bridge.killProcess.mockRejectedValue({ code: 'NOT_FOUND', message: 'process 999' })
    const store = useSystemStore()
    await store.refreshProcesses()
    await store.requestKillProcess(999, false)
    expect(store.error).toBe('process 999')
    expect(store.processes).toHaveLength(3)
  })
})
