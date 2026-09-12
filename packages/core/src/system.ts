import type { PortInfo, ProcessInfo } from '@dev-workbench/shared'

/** The native capabilities the system surfaces need. */
export interface SystemBridge {
  listProcesses(): Promise<ProcessInfo[]>
  processTree(pid: number): Promise<ProcessInfo[]>
  killProcess(pid: number): Promise<void>
  listPorts(): Promise<PortInfo[]>
  killPort(port: number): Promise<void>
}

export class SystemService {
  constructor(private readonly bridge: SystemBridge) {}

  listProcesses(): Promise<ProcessInfo[]> {
    return this.bridge.listProcesses()
  }

  processTree(pid: number): Promise<ProcessInfo[]> {
    return this.bridge.processTree(pid)
  }

  killProcess(pid: number): Promise<void> {
    return this.bridge.killProcess(pid)
  }

  listPorts(): Promise<PortInfo[]> {
    return this.bridge.listPorts()
  }

  killPort(port: number): Promise<void> {
    return this.bridge.killPort(port)
  }
}

function contains(value: string | undefined, needle: string): boolean {
  return value !== undefined && value.toLocaleLowerCase().includes(needle)
}

/** Case-insensitive match over the fields the process list shows. */
export function matchesProcess(process: ProcessInfo, query: string): boolean {
  const needle = query.trim().toLocaleLowerCase()
  if (!needle) return true
  return contains(process.name, needle)
    || contains(process.command, needle)
    || contains(process.executable, needle)
    || String(process.pid).includes(needle)
    || (process.parentPid !== undefined && String(process.parentPid).includes(needle))
}

/** Case-insensitive match over port, address, and owning process. */
export function matchesPort(port: PortInfo, query: string): boolean {
  const needle = query.trim().toLocaleLowerCase()
  if (!needle) return true
  return String(port.port).includes(needle)
    || contains(port.processName, needle)
    || contains(port.address, needle)
    || (port.pid !== undefined && String(port.pid).includes(needle))
}

/** Heaviest process first; pid breaks ties so the order stays stable. */
export function sortProcessesByMemory(processes: ProcessInfo[]): ProcessInfo[] {
  return [...processes].sort((left, right) =>
    (right.memoryBytes ?? 0) - (left.memoryBytes ?? 0) || left.pid - right.pid)
}

export function totalMemoryBytes(processes: ProcessInfo[]): number {
  return processes.reduce((total, process) => total + (process.memoryBytes ?? 0), 0)
}

/** Order processes so every child directly follows its parent. */
export function treeOrder(processes: ProcessInfo[]): ProcessInfo[] {
  const known = new Map(processes.map((process) => [process.pid, process]))
  const children = new Map<number, ProcessInfo[]>()
  const roots: ProcessInfo[] = []
  for (const process of processes) {
    const parent = process.parentPid
    if (parent === undefined || parent === process.pid || !known.has(parent)) {
      roots.push(process)
      continue
    }
    const siblings = children.get(parent) ?? []
    siblings.push(process)
    children.set(parent, siblings)
  }

  const ordered: ProcessInfo[] = []
  const visited = new Set<number>()
  const visit = (process: ProcessInfo, depth: number): void => {
    if (visited.has(process.pid) || depth > 32) return
    visited.add(process.pid)
    ordered.push(process)
    for (const child of (children.get(process.pid) ?? []).sort((left, right) => left.pid - right.pid)) {
      visit(child, depth + 1)
    }
  }
  for (const root of sortProcessesByMemory(roots)) visit(root, 0)
  // Anything left belongs to a parent cycle; keep it visible rather than hiding it.
  for (const process of sortProcessesByMemory(processes)) visit(process, 0)
  return ordered
}

/** Depth of each process in the parent/child forest, capped for rendering. */
export function processDepths(processes: ProcessInfo[]): Map<number, number> {
  const known = new Map(processes.map((process) => [process.pid, process]))
  const depths = new Map<number, number>()
  const resolve = (pid: number, seen: Set<number>): number => {
    const cached = depths.get(pid)
    if (cached !== undefined) return cached
    const parent = known.get(pid)?.parentPid
    if (parent === undefined || parent === pid || !known.has(parent) || seen.has(parent)) {
      depths.set(pid, 0)
      return 0
    }
    seen.add(pid)
    const depth = Math.min(resolve(parent, seen), 8) + 1
    depths.set(pid, depth)
    return depth
  }
  for (const process of processes) resolve(process.pid, new Set())
  return depths
}
