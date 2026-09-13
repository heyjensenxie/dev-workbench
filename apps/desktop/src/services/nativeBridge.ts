import { invoke } from '@tauri-apps/api/core'
import type { UnlistenFn } from '@tauri-apps/api/event'
import { listen } from '@tauri-apps/api/event'
import type { NativeBridge, VaultBridge } from '@dev-workbench/core'
import type { ApiModule, ConnectionTestResult, DatabaseConnectionConfig, DatabaseInfo, DevService, FileEngineStatus, HttpRequest, HttpResponse, ImagesToPdfRequest, PdfCompressionRequest, PdfCompressionResult, PdfMergeRequest, PdfSplitRequest, PdfToWordRequest, PortInfo, ProcessInfo, Project, ProjectContext, QueryResult, RunningService, SavedApiRequest, ServiceLogEvent, ServiceState, TableInfo, VaultBackupSummary, VaultGeneratorOptions, VaultImportOutcome, VaultItem, VaultItemPayload, VaultItemSummary, VaultLockReason, VaultStatus, WordToPdfRequest } from '@dev-workbench/shared'

/**
 * The vault bridge. Kept as its own object so that vault access is always an
 * explicit, greppable call (`nativeBridge.vault.…`) and cannot be mistaken for
 * ordinary workspace data access.
 *
 * Note that no method here can return a key, a list of decrypted secrets, or a
 * plaintext export. The master password is passed as a transient argument to
 * `unlock`/`create` and is never stored on this side.
 */
const vault: VaultBridge = {
  status: () => invoke<VaultStatus>('vault_status'),
  create: (masterPassword: string) => invoke<VaultStatus>('vault_create', { masterPassword }),
  unlock: (masterPassword: string) => invoke<VaultStatus>('vault_unlock', { masterPassword }),
  lock: () => invoke<boolean>('vault_lock'),
  touch: () => invoke<void>('vault_touch'),
  setAutoLock: (seconds: number) => invoke<void>('vault_set_auto_lock', { seconds }),
  listItems: () => invoke<VaultItemSummary[]>('vault_list_items'),
  getItem: async (id: string) => (await invoke<VaultItem | null>('vault_get_item', { id })) ?? undefined,
  createItem: (payload: VaultItemPayload) => invoke<VaultItem>('vault_create_item', { payload }),
  updateItem: (id: string, payload: VaultItemPayload) => invoke<VaultItem>('vault_update_item', { id, payload }),
  deleteItem: (id: string) => invoke<boolean>('vault_delete_item', { id }),
  destroy: () => invoke<void>('vault_destroy'),
  setFavorite: (id: string, favorite: boolean) => invoke<VaultItem>('vault_set_favorite', { id, favorite }),
  revealField: async (id: string, fieldIndex?: number) =>
    (await invoke<string | null>('vault_reveal_field', { id, fieldIndex: fieldIndex ?? null })) ?? undefined,
  copyItemField: (id: string, fieldIndex: number | undefined, clearAfterSeconds: number) =>
    invoke<void>('vault_copy_item_field', { id, fieldIndex: fieldIndex ?? null, clearAfterSeconds }),
  changeMasterPassword: (currentPassword: string, newPassword: string) =>
    invoke<void>('vault_change_master_password', { currentPassword, newPassword }),
  recalibrate: (masterPassword: string) => invoke<VaultStatus>('vault_recalibrate', { masterPassword }),
  exportBackup: (path: string) => invoke<VaultBackupSummary>('vault_export_backup', { path }),
  importBackup: (path: string, replaceExisting: boolean) =>
    invoke<VaultImportOutcome>('vault_import_backup', { path, replaceExisting }),
  generatePassword: (options?: Partial<VaultGeneratorOptions>) =>
    invoke<string>('vault_generate_password', { options: options ?? null }),
  copySecret: (value: string, clearAfterSeconds: number) =>
    invoke<void>('vault_copy_secret', { value, clearAfterSeconds }),
  clearClipboard: () => invoke<void>('vault_clear_clipboard'),
  openUrl: (url: string) => invoke<void>('vault_open_url', { url }),
}

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
  httpRequest = (request: HttpRequest) => invoke<HttpResponse>('http_request', { request })
  fileEngineStatus = () => invoke<FileEngineStatus>('file_engine_status')
  compressPdf = (request: PdfCompressionRequest) => invoke<PdfCompressionResult>('compress_pdf', { request })
  mergePdfs = (request: PdfMergeRequest) => invoke<PdfCompressionResult>('merge_pdfs', { request })
  splitPdf = (request: PdfSplitRequest) => invoke<PdfCompressionResult>('split_pdf', { request })
  pdfToWord = (request: PdfToWordRequest) => invoke<PdfCompressionResult>('pdf_to_word', { request })
  wordToPdf = (request: WordToPdfRequest) => invoke<PdfCompressionResult>('word_to_pdf', { request })
  imagesToPdf = (request: ImagesToPdfRequest) => invoke<PdfCompressionResult>('images_to_pdf', { request })
  writeFileBytes = (path: string, content: number[]) => invoke<void>('write_file_bytes', { path, content })
  listApiModules = () => invoke<ApiModule[]>('list_api_modules')
  saveApiModule = (module: ApiModule) => invoke<ApiModule>('save_api_module', { module })
  deleteApiModule = (moduleId: string) => invoke<boolean>('delete_api_module', { moduleId })
  listApiRequests = (moduleId?: string) => invoke<SavedApiRequest[]>('list_api_requests', { moduleId })
  saveApiRequest = (request: SavedApiRequest) => invoke<SavedApiRequest>('save_api_request', { request })
  deleteApiRequest = (requestId: string) => invoke<boolean>('delete_api_request', { requestId })
  testDatabaseConnection = (config: DatabaseConnectionConfig) => invoke<ConnectionTestResult>('test_database_connection', { config })
  listDatabaseDatabases = (config: DatabaseConnectionConfig) => invoke<DatabaseInfo[]>('list_database_databases', { config })
  listDatabaseTables = (config: DatabaseConnectionConfig, database: string) => invoke<TableInfo[]>('list_database_tables', { config, database })
  queryDatabase = (config: DatabaseConnectionConfig, sql: string) => invoke<QueryResult>('query_database', { config, sql })
  writeDatabaseExport = (path: string, content: string) => invoke<void>('write_database_export', { path, content })
  storeDatabasePassword = (connectionId: string, password: string) => invoke<void>('store_database_password', { connectionId, password })
  readDatabasePassword = async (connectionId: string) => (await invoke<string | null>('read_database_password', { connectionId })) ?? undefined
  deleteDatabasePassword = (connectionId: string) => invoke<void>('delete_database_password', { connectionId })
  vault = vault
}

export const nativeBridge = new TauriNativeBridge()

export const subscribeToServiceLogs = (handler: (event: ServiceLogEvent) => void): Promise<UnlistenFn> =>
  listen<ServiceLogEvent>('service-log', ({ payload }) => handler(payload))

/** Fires when a started service ends, however it ended. */
export const subscribeToServiceExit = (handler: (serviceId: string) => void): Promise<UnlistenFn> =>
  listen<string>('service-exit', ({ payload }) => handler(payload))

/**
 * Fires when the native vault supervisor locks the vault — on idle timeout, or
 * on resuming after the machine slept. The UI must drop every decrypted value it
 * is holding when this arrives.
 */
export const subscribeToVaultLock = (handler: (reason: VaultLockReason) => void): Promise<UnlistenFn> =>
  listen<VaultLockReason>('vault-locked', ({ payload }) => handler(payload))
