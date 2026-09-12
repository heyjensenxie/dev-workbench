import type { PortInfo, ProcessInfo } from '@dev-workbench/shared'
import { AppError, formatBytes } from '@dev-workbench/shared'
import {
  matchesPort,
  matchesProcess,
  processDepths as computeProcessDepths,
  totalMemoryBytes,
  treeOrder,
} from '@dev-workbench/core'
import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { workbench } from '../services/workbench'

export type ProcessOrder = 'memory' | 'tree'
export type ProcessSort = 'memory' | 'cpu' | 'pid' | 'name' | 'started'

const PROCESS_LIMIT = 300

export const useSystemStore = defineStore('system', () => {
  const processes = ref<ProcessInfo[]>([])
  const ports = ref<PortInfo[]>([])
  const processQuery = ref('')
  const portQuery = ref('')
  const processOrder = ref<ProcessOrder>('memory')
  const processSort = ref<ProcessSort>('memory')
  const processTree = ref(false)
  const loadingProcesses = ref(false)
  const loadingPorts = ref(false)
  const error = ref<string>()
  const pendingKill = ref<string>()

  const filteredProcesses = computed(() => {
    const matched = processes.value.filter((process) => matchesProcess(process, processQuery.value))
    if (processTree.value || processOrder.value === 'tree') return treeOrder(matched)
    return sortProcesses(matched, processSort.value)
  })
  const visibleProcesses = computed(() => filteredProcesses.value.slice(0, PROCESS_LIMIT))
  const truncatedProcesses = computed(() => Math.max(0, filteredProcesses.value.length - PROCESS_LIMIT))
  const processDepths = computed(() =>
    processTree.value || processOrder.value === 'tree' ? computeProcessDepths(visibleProcesses.value) : new Map<number, number>())
  const processesMemory = computed(() => formatBytes(totalMemoryBytes(filteredProcesses.value)))
  const filteredPorts = computed(() =>
    ports.value.filter((port) => matchesPort(port, portQuery.value)))

  function reportError(cause: unknown): void {
    error.value = AppError.from(cause).message
  }

  function dismissError(): void {
    error.value = undefined
  }

  function setProcessTree(enabled: boolean): void {
    processTree.value = enabled
    // Keep the old order flag in sync for consumers that persisted the original UI state.
    processOrder.value = enabled ? 'tree' : 'memory'
  }

  async function refreshProcesses(): Promise<void> {
    loadingProcesses.value = true
    try {
      processes.value = await workbench.system.listProcesses()
    } catch (cause) {
      reportError(cause)
    } finally {
      loadingProcesses.value = false
    }
  }

  async function refreshPorts(): Promise<void> {
    loadingPorts.value = true
    try {
      ports.value = await workbench.system.listPorts()
    } catch (cause) {
      reportError(cause)
    } finally {
      loadingPorts.value = false
    }
  }

  async function refresh(): Promise<void> {
    await Promise.all([refreshProcesses(), refreshPorts()])
  }

  async function killProcess(pid: number): Promise<void> {
    error.value = undefined
    try {
      await workbench.system.killProcess(pid)
      await refreshProcesses()
    } catch (cause) {
      reportError(cause)
    }
  }

  async function freePort(port: number): Promise<void> {
    error.value = undefined
    try {
      await workbench.system.killPort(port)
      await Promise.all([refreshPorts(), refreshProcesses()])
    } catch (cause) {
      reportError(cause)
    }
  }

  /** Runs `action` when confirmation is disabled, otherwise arms `key` first. */
  async function requestConfirmation(key: string, confirm: boolean, action: () => Promise<void>): Promise<void> {
    if (!confirm || pendingKill.value === key) {
      pendingKill.value = undefined
      await action()
      return
    }
    pendingKill.value = key
  }

  async function requestKillProcess(pid: number, confirm: boolean): Promise<void> {
    await requestConfirmation(`process:${pid}`, confirm, () => killProcess(pid))
  }

  async function requestFreePort(port: number, confirm: boolean): Promise<void> {
    await requestConfirmation(`port:${port}`, confirm, () => freePort(port))
  }

  function cancelPendingKill(): void {
    pendingKill.value = undefined
  }

  return {
    processes, ports, processQuery, portQuery, processOrder, processSort, processTree, loadingProcesses, loadingPorts,
    error, pendingKill, filteredProcesses, visibleProcesses, truncatedProcesses, processDepths,
    processesMemory, filteredPorts,
    refresh, refreshProcesses, refreshPorts, killProcess, freePort,
    requestKillProcess, requestFreePort, cancelPendingKill, dismissError, setProcessTree,
  }
})

function sortProcesses(processes: ProcessInfo[], sort: ProcessSort): ProcessInfo[] {
  return [...processes].sort((left, right) => {
    if (sort === 'cpu') return (right.cpuPercent ?? 0) - (left.cpuPercent ?? 0) || left.pid - right.pid
    if (sort === 'pid') return left.pid - right.pid
    if (sort === 'name') return left.name.localeCompare(right.name) || left.pid - right.pid
    if (sort === 'started') return (right.startedAt ?? 0) - (left.startedAt ?? 0) || left.pid - right.pid
    return (right.memoryBytes ?? 0) - (left.memoryBytes ?? 0) || left.pid - right.pid
  })
}
