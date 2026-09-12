import { invoke } from '@tauri-apps/api/core'
import type { UnlistenFn } from '@tauri-apps/api/event'
import { listen } from '@tauri-apps/api/event'
import type { NativeBridge } from '@dev-workbench/core'
import type { DevService, PortInfo, ProcessInfo, Project, ProjectContext, RunningService, ServiceLogEvent, ServiceState } from '@dev-workbench/shared'

class TauriNativeBridge implements NativeBridge {
  listProjects = () => invoke<Project[]>('list_projects')
  openProject = (path: string) => invoke<Project>('open_project', { path })
  deleteProject = (projectId: string) => invoke<boolean>('delete_project', { projectId })
  revealProject = (path: string) => invoke<void>('reveal_project', { path })
  scanProject = (project: Project) => invoke<ProjectContext>('scan_project', { project })
  listServices = (projectId: string) => invoke<DevService[]>('list_services', { projectId })
  saveService = (service: DevService) => invoke<void>('save_service', { service })
  deleteService = (serviceId: string) => invoke<boolean>('delete_service', { serviceId })
  startService = (service: DevService) => invoke<ServiceState>('start_service', { service })
  stopService = (serviceId: string) => invoke<ServiceState>('stop_service', { serviceId })
  stopAllServices = () => invoke<number>('stop_all_services')
  listRunningServices = () => invoke<RunningService[]>('list_running_services')
  listProcesses = () => invoke<ProcessInfo[]>('list_processes')
  processTree = (pid: number) => invoke<ProcessInfo[]>('process_tree', { pid })
  killProcess = (pid: number) => invoke<void>('kill_process', { pid })
  listPorts = () => invoke<PortInfo[]>('list_ports')
  killPort = (port: number) => invoke<void>('kill_port', { port })
  getSettings = () => invoke<Record<string, unknown>>('get_settings')
  setSetting = (key: string, value: unknown) => invoke<void>('set_setting', { key, value })
}

export const nativeBridge = new TauriNativeBridge()

export const subscribeToServiceLogs = (handler: (event: ServiceLogEvent) => void): Promise<UnlistenFn> =>
  listen<ServiceLogEvent>('service-log', ({ payload }) => handler(payload))

/** Fires when a started service ends, however it ended. */
export const subscribeToServiceExit = (handler: (serviceId: string) => void): Promise<UnlistenFn> =>
  listen<string>('service-exit', ({ payload }) => handler(payload))
