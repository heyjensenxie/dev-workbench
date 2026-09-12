export type DatabaseType = 'mysql' | 'sqlite' | 'postgresql'

/**
 * Locale-neutral codes for text this workbench writes itself (as opposed to
 * wording produced by a database engine or driver). Both the native runtime and
 * the browser preview emit these codes, and the presentation layer translates
 * them; any unrecognized message is shown verbatim so real engine errors keep
 * their original wording.
 */
export const DATABASE_MESSAGE_CODES = {
  connectedMysql: 'database.message.connectedMysql',
  connectedSqlite: 'database.message.connectedSqlite',
  statementExecuted: 'database.message.statementExecuted',
  queryCancelled: 'database.message.queryCancelled',
  limitReached: 'database.message.limitReached',
  unknownHost: 'database.message.unknownHost',
} as const

export type DatabaseMessageCode = (typeof DATABASE_MESSAGE_CODES)[keyof typeof DATABASE_MESSAGE_CODES]

export type DatabaseConnectionStatus = 'disconnected' | 'connecting' | 'connected' | 'failed'

export interface DatabaseConnection {
  id: string
  name: string
  type: DatabaseType
  host?: string
  port?: number
  username?: string
  database?: string | undefined
  sqlitePath?: string
  secretRef?: string | undefined
  projectId?: string
  options?: Record<string, unknown>
  createdAt: number
  updatedAt: number
}

export interface DatabaseConnectionConfig extends Omit<DatabaseConnection, 'createdAt' | 'updatedAt'> {
  password?: string
  connectionTimeoutMs?: number
  sslMode?: 'disable' | 'prefer' | 'require'
}

export interface ConnectionTestResult {
  success: boolean
  latencyMs?: number
  serverVersion?: string
  message: string
  errorCode?: string
}

export interface DatabaseInfo {
  name: string
  type?: string
}

export interface SchemaInfo {
  name: string
}

export interface TableInfo {
  name: string
  type: 'table' | 'view'
  rows?: number
  database?: string | undefined
}

export interface TableRef {
  database?: string | undefined
  table: string
}

export interface ColumnInfo {
  name: string
  type: string
  nullable: boolean
  defaultValue?: string | null | undefined
  primaryKey?: boolean | undefined
  extra?: string | undefined
  comment?: string | undefined
}

export interface IndexInfo {
  name: string
  columns: string[]
  type: string
  unique: boolean
}

export interface QueryColumn {
  name: string
  type: string
}

export interface QueryOptions {
  timeoutMs?: number
  limit?: number
  signal?: AbortSignal
}

export interface QueryResult {
  columns: QueryColumn[]
  rows: Record<string, unknown>[]
  affectedRows?: number
  executionTime: number
  truncated?: boolean
  message?: string | undefined
}

export interface ExplainResult {
  columns: QueryColumn[]
  rows: Record<string, unknown>[]
  warnings: string[]
  executionTime: number
}

export interface DatabaseSession {
  query(sql: string, options?: QueryOptions): Promise<QueryResult>
  cancel?(): Promise<void>
  listDatabases(): Promise<DatabaseInfo[]>
  listSchemas?(): Promise<SchemaInfo[]>
  listTables(database?: string): Promise<TableInfo[]>
  getTableColumns(table: TableRef): Promise<ColumnInfo[]>
  getIndexes(table: TableRef): Promise<IndexInfo[]>
  getCreateTable(table: TableRef): Promise<string>
  explain?(sql: string): Promise<ExplainResult>
  close(): Promise<void>
}

export interface DatabaseAdapter {
  type: DatabaseType
  connect(config: DatabaseConnectionConfig): Promise<DatabaseSession>
  testConnection(config: DatabaseConnectionConfig): Promise<ConnectionTestResult>
}

export interface DatabaseHistoryEntry {
  id: string
  connectionId: string
  database?: string | undefined
  sqlText: string
  success: boolean
  errorCode?: string | undefined
  executionTime: number
  executedAt: number
}

export interface DatabaseContext {
  connectionId?: string | undefined
  type?: DatabaseType | undefined
  database?: string | undefined
  selectedTable?: string | undefined
  activeQuery?: string | undefined
}
