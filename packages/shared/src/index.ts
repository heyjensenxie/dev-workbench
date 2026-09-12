export type ErrorCode =
  | 'VALIDATION_ERROR'
  | 'NOT_FOUND'
  | 'CONFLICT'
  | 'NATIVE_ERROR'
  | 'COMMAND_ERROR'
  | 'UNKNOWN_ERROR'

export interface SerializedAppError {
  code: ErrorCode
  message: string
  details?: Record<string, unknown>
}

export class AppError extends Error {
  constructor(
    public readonly code: ErrorCode,
    message: string,
    public readonly details?: Record<string, unknown>,
    options?: ErrorOptions,
  ) {
    super(message, options)
    this.name = 'AppError'
  }

  static from(error: unknown, fallbackMessage = 'An unexpected error occurred'): AppError {
    if (error instanceof AppError) return error
    if (typeof error === 'object' && error !== null && 'code' in error && 'message' in error) {
      const candidate = error as SerializedAppError
      return new AppError(candidate.code, candidate.message, candidate.details)
    }
    return new AppError('UNKNOWN_ERROR', fallbackMessage, undefined, { cause: error })
  }
}

export interface Project {
  id: string
  name: string
  path: string
  createdAt: number
  updatedAt: number
  lastOpenedAt?: number
}

export interface DetectedFile {
  path: string
  kind: string
}

/** A runnable script declared by a project manifest. */
export interface DetectedScript {
  name: string
  command: string
  source: string
}

/** A service the scanner believes the project can start. */
export interface SuggestedService {
  name: string
  command: string
  args: string[]
  cwd?: string
  port?: number
  source: string
}

export interface VcsInfo {
  kind: string
  branch?: string
}

export interface ProjectContext {
  id: string
  name: string
  path: string
  description?: string
  languages: string[]
  frameworks: string[]
  packageManagers: string[]
  detectedFiles: DetectedFile[]
  scripts: DetectedScript[]
  suggestedServices: SuggestedService[]
  composeServices: string[]
  monorepo: boolean
  vcs?: VcsInfo
  scannedAt: number
}

export interface DevService {
  id: string
  projectId: string
  name: string
  cwd?: string
  command: string
  args?: string[]
  env?: Record<string, string>
  port?: number
  autoOpen?: boolean
  dependencies?: string[]
  updatedAt?: number
}

export type ServiceStatus = 'stopped' | 'starting' | 'running' | 'stopping' | 'failed'

export interface ServiceState {
  serviceId: string
  status: ServiceStatus
  pid?: number
  error?: SerializedAppError
}

export interface RunningService {
  serviceId: string
  pid: number
}

export interface ProcessInfo {
  pid: number
  name: string
  command?: string
  parentPid?: number
  executable?: string
  /** Resident set size in bytes. */
  memoryBytes?: number
  /** Recent CPU utilization percentage reported by the operating system. */
  cpuPercent?: number
  /** Seconds since the Unix epoch. */
  startedAt?: number
}

export interface PortInfo {
  port: number
  pid?: number
  processName?: string
  address: string
}

export interface ServiceLogEvent {
  serviceId: string
  stream: 'stdout' | 'stderr'
  line: string
  timestamp: number
}

export type HttpMethod = 'GET' | 'POST' | 'PUT' | 'PATCH' | 'DELETE' | 'HEAD' | 'OPTIONS'

/** In-memory request contract shared by the API editor and native HTTP adapter. */
export interface HttpRequest {
  method: HttpMethod
  url: string
  headers: Record<string, string>
  body?: string
  timeoutMs?: number
}

/** Response metadata returned to the local API editor without native logging. */
export interface HttpResponse {
  status: number
  statusText: string
  headers: Record<string, string>
  body: string
  durationMs: number
  truncated: boolean
}

/** Persisted API request template; environment values are intentionally not included. */
export interface SavedApiRequest {
  id: string
  name: string
  method: HttpMethod
  url: string
  moduleId?: string
  headers: Record<string, string>
  body?: string
  createdAt: number
  updatedAt: number
}

export interface ApiModule {
  id: string
  name: string
  description?: string
  createdAt: number
  updatedAt: number
}

/** Field-level problems a service definition can have. */
export type ServiceIssue =
  | 'nameRequired'
  | 'nameTooLong'
  | 'commandRequired'
  | 'selfDependency'

/**
 * Mirrors the validation enforced by the native repository, so the editor can
 * point at the offending field before anything is persisted. A working
 * directory may be absolute or relative to the owning project root; the
 * native runtime resolves the latter immediately before spawning.
 */
export function validateDevService(service: DevService): ServiceIssue[] {
  const issues: ServiceIssue[] = []
  const name = service.name?.trim() ?? ''
  if (!name) issues.push('nameRequired')
  else if (name.length > 80) issues.push('nameTooLong')
  if (!service.command?.trim()) issues.push('commandRequired')
  if ((service.dependencies ?? []).includes(service.id)) issues.push('selfDependency')
  return issues
}

/** Absolute on POSIX, drive-lettered or UNC on Windows. */
export function isAbsolutePath(path: string): boolean {
  return path.startsWith('/') || path.startsWith('\\\\') || /^[a-zA-Z]:[\\/]/.test(path)
}

/** Generates an id for a service that has not been persisted yet. */
export function createServiceId(): string {
  const random = globalThis.crypto?.randomUUID?.()
  return random ?? `service-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`
}

export type ThemeName = 'system' | 'dark' | 'light'
export type LocaleName = 'zh-CN' | 'en-US'

/** Preferences persisted in the `settings` table. */
export interface WorkbenchSettings {
  theme: ThemeName
  locale: LocaleName
  /** Restore the most recently opened project after startup. */
  openLastProject: boolean
  /** Maximum number of log lines kept in memory. */
  logLimit: number
  /** Ask before terminating a process or freeing a port. */
  confirmBeforeKill: boolean
}

export const LOG_LIMIT_MIN = 100
export const LOG_LIMIT_MAX = 10_000

/**
 * Keeps only the persisted settings that are present and usable, so callers can
 * fall back to their own defaults per field.
 */
export function parseSettings(raw: Record<string, unknown>): Partial<WorkbenchSettings> {
  const settings: Partial<WorkbenchSettings> = {}
  if (raw.theme === 'system' || raw.theme === 'dark' || raw.theme === 'light') settings.theme = raw.theme
  if (raw.locale === 'zh-CN' || raw.locale === 'en-US') settings.locale = raw.locale
  if (typeof raw.openLastProject === 'boolean') settings.openLastProject = raw.openLastProject
  if (typeof raw.logLimit === 'number' && Number.isFinite(raw.logLimit)) {
    settings.logLimit = clampLogLimit(Math.round(raw.logLimit))
  }
  if (typeof raw.confirmBeforeKill === 'boolean') settings.confirmBeforeKill = raw.confirmBeforeKill
  return settings
}

export function clampLogLimit(value: number): number {
  return Math.min(LOG_LIMIT_MAX, Math.max(LOG_LIMIT_MIN, value))
}

/** Human-readable byte size, e.g. `1.4 GB`. */
export function formatBytes(bytes: number | undefined): string {
  if (bytes === undefined || !Number.isFinite(bytes) || bytes <= 0) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB', 'TB']
  let value = bytes
  let unit = 0
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024
    unit += 1
  }
  const digits = value >= 100 || unit === 0 ? 0 : 1
  return `${value.toFixed(digits)} ${units[unit]}`
}

/** Compact elapsed time, e.g. `3h 12m` or `45s`. */
export function formatUptime(startedAtSeconds: number | undefined, now = Date.now()): string {
  if (!startedAtSeconds || !Number.isFinite(startedAtSeconds)) return '—'
  const seconds = Math.max(0, Math.floor(now / 1000 - startedAtSeconds))
  if (seconds < 60) return `${seconds}s`
  const minutes = Math.floor(seconds / 60)
  if (minutes < 60) return `${minutes}m`
  const hours = Math.floor(minutes / 60)
  if (hours < 24) return `${hours}h ${minutes % 60}m`
  return `${Math.floor(hours / 24)}d ${hours % 24}h`
}
