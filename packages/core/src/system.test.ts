import type { PortInfo, ProcessInfo } from '@dev-workbench/shared'
import { describe, expect, it, vi } from 'vitest'
import {
  matchesPort,
  matchesProcess,
  processDepths,
  sortProcessesByMemory,
  SystemService,
  totalMemoryBytes,
  treeOrder,
} from './system'
import type { SystemBridge } from './system'

function process(overrides: Partial<ProcessInfo> & { pid: number }): ProcessInfo {
  return { name: `proc-${overrides.pid}`, ...overrides }
}

function createBridge(overrides: Partial<SystemBridge> = {}): SystemBridge {
  return {
    listProcesses: vi.fn(async () => [] as ProcessInfo[]),
    processTree: vi.fn(async () => [] as ProcessInfo[]),
    killProcess: vi.fn(async () => undefined),
    listPorts: vi.fn(async () => [] as PortInfo[]),
    killPort: vi.fn(async () => undefined),
    ...overrides,
  }
}

describe('SystemService', () => {
  it('delegates every call to the bridge', async () => {
    const bridge = createBridge({
      listProcesses: vi.fn(async () => [process({ pid: 1 })]),
      listPorts: vi.fn(async () => [{ port: 5173, address: '127.0.0.1:5173' }]),
    })
    const system = new SystemService(bridge)

    await expect(system.listProcesses()).resolves.toHaveLength(1)
    await expect(system.listPorts()).resolves.toEqual([{ port: 5173, address: '127.0.0.1:5173' }])
    await system.killProcess(4242)
    await system.killPort(5173)
    await system.processTree(4242)

    expect(bridge.killProcess).toHaveBeenCalledWith(4242)
    expect(bridge.killPort).toHaveBeenCalledWith(5173)
    expect(bridge.processTree).toHaveBeenCalledWith(4242)
  })
})

describe('matchesProcess', () => {
  const vite = process({
    pid: 4242,
    name: 'node',
    command: 'node vite.js --port 5173',
    executable: 'C:\\Program Files\\nodejs\\node.exe',
    parentPid: 100,
  })

  it('matches an empty query and any visible field', () => {
    expect(matchesProcess(vite, '  ')).toBe(true)
    expect(matchesProcess(vite, 'NODE')).toBe(true)
    expect(matchesProcess(vite, 'vite.js')).toBe(true)
    expect(matchesProcess(vite, 'program files')).toBe(true)
    expect(matchesProcess(vite, '42')).toBe(true)
    expect(matchesProcess(vite, '100')).toBe(true)
    expect(matchesProcess(vite, 'postgres')).toBe(false)
  })
})

describe('matchesPort', () => {
  const port: PortInfo = { port: 5173, pid: 4242, processName: 'node', address: '127.0.0.1:5173' }

  it('matches port, address, process, and pid', () => {
    expect(matchesPort(port, '')).toBe(true)
    expect(matchesPort(port, '517')).toBe(true)
    expect(matchesPort(port, '127.0.0')).toBe(true)
    expect(matchesPort(port, 'Node')).toBe(true)
    expect(matchesPort(port, '4242')).toBe(true)
    expect(matchesPort(port, '8080')).toBe(false)
  })

  it('handles ports without an owning process', () => {
    expect(matchesPort({ port: 53, address: '0.0.0.0:53' }, '53')).toBe(true)
  })
})

describe('memory helpers', () => {
  it('sorts heaviest first and breaks ties by pid', () => {
    const sorted = sortProcessesByMemory([
      process({ pid: 3, memoryBytes: 10 }),
      process({ pid: 1, memoryBytes: 30 }),
      process({ pid: 2, memoryBytes: 10 }),
    ])
    expect(sorted.map((item) => item.pid)).toEqual([1, 2, 3])
  })

  it('treats a missing memory reading as zero', () => {
    expect(totalMemoryBytes([process({ pid: 1, memoryBytes: 5 }), process({ pid: 2 })])).toBe(5)
  })
})

describe('treeOrder', () => {
  it('keeps every child directly after its parent', () => {
    const ordered = treeOrder([
      process({ pid: 3, parentPid: 2 }),
      process({ pid: 1 }),
      process({ pid: 2, parentPid: 1 }),
    ])
    expect(ordered.map((item) => item.pid)).toEqual([1, 2, 3])
  })

  it('treats processes with unknown or self parents as roots', () => {
    const ordered = treeOrder([
      process({ pid: 5, parentPid: 999 }),
      process({ pid: 6, parentPid: 6 }),
    ])
    expect(ordered.map((item) => item.pid).sort()).toEqual([5, 6])
  })

  it('still lists processes trapped in a parent cycle', () => {
    const ordered = treeOrder([
      process({ pid: 7, parentPid: 8 }),
      process({ pid: 8, parentPid: 7 }),
    ])
    expect(ordered.map((item) => item.pid).sort()).toEqual([7, 8])
  })

  it('orders roots by memory', () => {
    const ordered = treeOrder([
      process({ pid: 1, memoryBytes: 1 }),
      process({ pid: 2, memoryBytes: 100 }),
    ])
    expect(ordered.map((item) => item.pid)).toEqual([2, 1])
  })
})

describe('processDepths', () => {
  it('reports nesting depth for children and grandchildren', () => {
    const depths = processDepths([
      process({ pid: 1 }),
      process({ pid: 2, parentPid: 1 }),
      process({ pid: 3, parentPid: 2 }),
    ])
    expect(depths.get(1)).toBe(0)
    expect(depths.get(2)).toBe(1)
    expect(depths.get(3)).toBe(2)
  })

  it('stops at a parent cycle instead of recursing forever', () => {
    const depths = processDepths([
      process({ pid: 1, parentPid: 2 }),
      process({ pid: 2, parentPid: 1 }),
    ])
    expect(depths.size).toBe(2)
    expect([...depths.values()].every((depth) => depth <= 8)).toBe(true)
  })
})
