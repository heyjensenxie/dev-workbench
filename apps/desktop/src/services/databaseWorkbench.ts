import type { ColumnInfo, ConnectionTestResult, DatabaseConnection, DatabaseConnectionConfig, DatabaseHistoryEntry, DatabaseInfo, DatabaseSession, ExplainResult, IndexInfo, QueryResult, TableInfo, TableRef } from '@dev-workbench/shared'
import { DATABASE_MESSAGE_CODES } from '@dev-workbench/shared'
import { analyzeSqlSafety, applyResultLimit, databaseLabel, inferColumnType, isReadQuery, splitSqlStatements, type NativeBridge } from '@dev-workbench/core'
import { nativeBridge } from './nativeBridge'

const CONNECTIONS_KEY = 'database-connections'
const HISTORY_KEY = 'database-query-history'
const MAX_HISTORY = 80

function createId(prefix: string): string {
  return `${prefix}-${globalThis.crypto?.randomUUID?.() ?? `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 9)}`}`
}

function readJson<T>(key: string, fallback: T): T {
  try {
    const value = localStorage.getItem(key)
    return value ? JSON.parse(value) as T : fallback
  } catch {
    return fallback
  }
}

function writeJson(key: string, value: unknown): void {
  try { localStorage.setItem(key, JSON.stringify(value)) } catch { /* browser preview persistence is best effort */ }
}

function quoteMysqlIdentifier(identifier: string): string {
  return `\`${identifier.replaceAll('`', '``')}\``
}

function tablesFromShowResult(result: QueryResult): TableInfo[] {
  const nameColumn = result.columns[0]?.name
  const typeColumn = result.columns.find((column) => column.name.toLocaleLowerCase() === 'table_type')?.name ?? result.columns[1]?.name
  if (!nameColumn) return []
  return result.rows.flatMap((row) => {
    const name = row[nameColumn]
    if (typeof name !== 'string' || !name) return []
    const rawType = typeColumn ? String(row[typeColumn] ?? '') : ''
    return [{ name, type: rawType.toLocaleUpperCase() === 'VIEW' ? 'view' as const : 'table' as const }]
  })
}

const mysqlTables: TableInfo[] = [
  { name: 'sys_user', type: 'table', rows: 1284 },
  { name: 'mcp_servers', type: 'table', rows: 18 },
  { name: 'tool_logs', type: 'table', rows: 9231 },
  { name: 'user_summary', type: 'view' },
]
const sqliteTables: TableInfo[] = [
  { name: 'projects', type: 'table', rows: 4 },
  { name: 'project_services', type: 'table', rows: 11 },
  { name: 'settings', type: 'table', rows: 6 },
]

const tableColumns: Record<string, ColumnInfo[]> = {
  sys_user: [
    { name: 'id', type: 'bigint', nullable: false, primaryKey: true, extra: 'auto_increment', comment: '用户主键' },
    { name: 'name', type: 'varchar(80)', nullable: false, comment: '显示名称' },
    { name: 'mobile', type: 'varchar(20)', nullable: true, comment: '手机号（脱敏）' },
    { name: 'status', type: 'tinyint', nullable: false, defaultValue: '1' },
    { name: 'created_at', type: 'datetime', nullable: false, defaultValue: 'CURRENT_TIMESTAMP' },
  ],
  mcp_servers: [
    { name: 'id', type: 'varchar(36)', nullable: false, primaryKey: true },
    { name: 'name', type: 'varchar(120)', nullable: false },
    { name: 'endpoint', type: 'varchar(255)', nullable: false },
    { name: 'enabled', type: 'tinyint(1)', nullable: false, defaultValue: '1' },
    { name: 'updated_at', type: 'datetime', nullable: false },
  ],
  projects: [
    { name: 'id', type: 'TEXT', nullable: false, primaryKey: true },
    { name: 'name', type: 'TEXT', nullable: false },
    { name: 'path', type: 'TEXT', nullable: false },
    { name: 'updated_at', type: 'INTEGER', nullable: false },
  ],
}

const tableIndexes: Record<string, IndexInfo[]> = {
  sys_user: [
    { name: 'PRIMARY', columns: ['id'], type: 'BTREE', unique: true },
    { name: 'idx_user_mobile', columns: ['mobile'], type: 'BTREE', unique: true },
    { name: 'idx_user_status_created', columns: ['status', 'created_at'], type: 'BTREE', unique: false },
  ],
  mcp_servers: [{ name: 'PRIMARY', columns: ['id'], type: 'BTREE', unique: true }],
  projects: [{ name: 'sqlite_autoindex_projects_1', columns: ['id'], type: 'BTREE', unique: true }],
}

const fixtureRows: Record<string, Record<string, unknown>[]> = {
  sys_user: [
    { id: 10001, name: 'Jensen', mobile: '138****1024', status: 1, created_at: '2026-08-22 09:14:05' },
    { id: 10002, name: 'Lin', mobile: '139****7781', status: 1, created_at: '2026-08-21 17:32:44' },
    { id: 10003, name: 'Mia', mobile: null, status: 0, created_at: '2026-08-17 11:06:19' },
    { id: 10004, name: 'Noah', mobile: '186****9920', status: 1, created_at: '2026-08-14 15:45:03' },
  ],
  mcp_servers: [
    { id: 'srv_01', name: 'Local tools', endpoint: 'http://localhost:8765/mcp', enabled: true, updated_at: '2026-08-20 13:22:10' },
    { id: 'srv_02', name: 'Docs search', endpoint: 'http://localhost:8770/mcp', enabled: false, updated_at: '2026-08-18 08:05:31' },
  ],
  projects: [
    { id: 'p-001', name: 'dev-workbench', path: 'F:/projects/node/dev-workbench', updated_at: 1726461900 },
    { id: 'p-002', name: 'mcp-conductor', path: 'F:/projects/node/mcp-conductor', updated_at: 1726321120 },
  ],
}

class PreviewDatabaseSession implements DatabaseSession {
  private cancelled = false
  constructor(private readonly connection: DatabaseConnection) {}

  async query(sql: string, options?: { limit?: number }): Promise<QueryResult> {
    const started = performance.now()
    await new Promise((resolve) => window.setTimeout(resolve, 180))
    if (this.cancelled) throw new Error(DATABASE_MESSAGE_CODES.queryCancelled)
    if (/syntax_error|selekt|from\s*;/i.test(sql)) throw Object.assign(new Error('You have an error in your SQL syntax near the supplied statement.'), { code: '1064' })
    const source = sql.match(/\bfrom\s+[`"']?([\w-]+)/i)?.[1] ?? 'sys_user'
    const rows = fixtureRows[source] ?? fixtureRows[this.connection.type === 'sqlite' ? 'projects' : 'sys_user'] ?? []
    if (isReadQuery(sql)) {
      const limit = options?.limit ?? 500
      const resultRows = /\blimit\s+(\d+)/i.exec(sql)?.[1]
      const requested = resultRows ? Number(resultRows) : limit
      const response: QueryResult = { columns: Object.keys(rows[0] ?? {}).map((name) => ({ name, type: inferColumnType(rows[0]?.[name]) })), rows: rows.slice(0, requested), executionTime: Math.round(performance.now() - started), truncated: rows.length > requested }
      if (rows.length > requested) response.message = DATABASE_MESSAGE_CODES.limitReached
      return response
    }
    return { columns: [], rows: [], affectedRows: 3, executionTime: Math.round(performance.now() - started), message: DATABASE_MESSAGE_CODES.statementExecuted }
  }

  async cancel(): Promise<void> { this.cancelled = true }
  async listDatabases(): Promise<DatabaseInfo[]> { return this.connection.type === 'sqlite' ? [{ name: 'main', type: 'SQLite' }] : [{ name: 'information_schema' }, { name: this.connection.database ?? 'mcp_conductor' }, { name: 'mysql' }, { name: 'sys' }] }
  async listTables(database?: string): Promise<TableInfo[]> { return (database === 'information_schema' || database === 'mysql' || database === 'sys') ? [] : this.connection.type === 'sqlite' ? sqliteTables : mysqlTables }
  async getTableColumns(table: TableRef): Promise<ColumnInfo[]> { return tableColumns[table.table] ?? [{ name: 'id', type: this.connection.type === 'sqlite' ? 'INTEGER' : 'bigint', nullable: false, primaryKey: true }, { name: 'value', type: 'TEXT', nullable: true }] }
  async getIndexes(table: TableRef): Promise<IndexInfo[]> { return tableIndexes[table.table] ?? [] }
  async getCreateTable(table: TableRef): Promise<string> {
    const columns = await this.getTableColumns(table)
    const body = columns.map((column) => `  ${column.name} ${column.type}${column.nullable ? '' : ' NOT NULL'}${column.primaryKey ? ' PRIMARY KEY' : ''}${column.defaultValue ? ` DEFAULT ${column.defaultValue}` : ''}`).join(',\n')
    return this.connection.type === 'sqlite' ? `CREATE TABLE ${table.table} (\n${body}\n);` : `CREATE TABLE \`${table.table}\` (\n${body}\n) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;`
  }
  async explain(sql: string): Promise<ExplainResult> {
    const started = performance.now()
    await new Promise((resolve) => window.setTimeout(resolve, 120))
    return { columns: [{ name: 'id', type: 'number' }, { name: 'select_type', type: 'text' }, { name: 'table', type: 'text' }, { name: 'type', type: 'text' }, { name: 'possible_keys', type: 'text' }, { name: 'key', type: 'text' }, { name: 'rows', type: 'number' }, { name: 'Extra', type: 'text' }], rows: [{ id: 1, select_type: 'SIMPLE', table: sql.match(/\bfrom\s+(\w+)/i)?.[1] ?? 'sys_user', type: 'ref', possible_keys: 'idx_user_status_created', key: 'idx_user_status_created', rows: 42, Extra: 'Using index condition' }], warnings: [], executionTime: Math.round(performance.now() - started) }
  }
  async close(): Promise<void> {}
}

export class DatabaseWorkbenchService {
  private readonly sessions = new Map<string, PreviewDatabaseSession>()
  private readonly passwords = new Map<string, string>()

  constructor(private readonly bridge: Pick<NativeBridge, 'testDatabaseConnection' | 'listDatabaseDatabases' | 'listDatabaseTables' | 'queryDatabase' | 'storeDatabasePassword' | 'readDatabasePassword' | 'deleteDatabasePassword'> = nativeBridge) {}

  listConnections(): DatabaseConnection[] { return readJson<DatabaseConnection[]>(CONNECTIONS_KEY, []) }
  saveConnection(config: DatabaseConnectionConfig, rememberPassword = true): DatabaseConnection {
    const now = Date.now()
    const current = this.listConnections().find((item) => item.id === config.id)
    const { password: _password, connectionTimeoutMs: _timeout, sslMode: _ssl, ...persistedConfig } = config
    const connection: DatabaseConnection = { ...persistedConfig, ...(config.password && rememberPassword ? { secretRef: `secret://database/${config.id}/password` } : current?.secretRef ? { secretRef: current.secretRef } : {}), createdAt: current?.createdAt ?? now, updatedAt: now }
    const connections = this.listConnections().filter((item) => item.id !== connection.id)
    writeJson(CONNECTIONS_KEY, [connection, ...connections])
    if (config.password && rememberPassword) this.passwords.set(connection.id, config.password)
    return connection
  }
  async persistPassword(connectionId: string, password: string, rememberPassword: boolean): Promise<void> {
    if (password && rememberPassword) {
      this.passwords.set(connectionId, password)
      if (this.hasNativeRuntime() && this.bridge.storeDatabasePassword) await this.bridge.storeDatabasePassword(connectionId, password)
    } else {
      this.passwords.delete(connectionId)
      if (this.hasNativeRuntime() && this.bridge.deleteDatabasePassword) await this.bridge.deleteDatabasePassword(connectionId)
    }
  }
  private async resolvePassword(connectionId: string): Promise<string | undefined> {
    const inMemory = this.passwords.get(connectionId)
    if (inMemory !== undefined) return inMemory
    if (!this.hasNativeRuntime() || !this.bridge.readDatabasePassword) return undefined
    const stored = await this.bridge.readDatabasePassword(connectionId)
    if (stored !== undefined) this.passwords.set(connectionId, stored)
    return stored
  }
  removeConnection(id: string): void { this.sessions.delete(id); this.passwords.delete(id); writeJson(CONNECTIONS_KEY, this.listConnections().filter((item) => item.id !== id)) }
  async testConnection(config: DatabaseConnectionConfig): Promise<ConnectionTestResult> {
    if (this.hasNativeRuntime() && this.bridge.testDatabaseConnection) return this.bridge.testDatabaseConnection(config)
    await new Promise((resolve) => window.setTimeout(resolve, 420))
    if (config.type === 'sqlite') return { success: true, latencyMs: 8, serverVersion: 'SQLite 3.45', message: DATABASE_MESSAGE_CODES.connectedSqlite }
    if (!config.host || config.host === 'localhost' || config.host === '127.0.0.1') return { success: true, latencyMs: 24, serverVersion: 'MySQL 8.0.36', message: DATABASE_MESSAGE_CODES.connectedMysql }
    return { success: false, message: DATABASE_MESSAGE_CODES.unknownHost, errorCode: 'ENOTFOUND' }
  }
  async listDatabases(connection: DatabaseConnection): Promise<DatabaseInfo[]> {
    const session = this.sessions.get(connection.id)
    if (this.hasNativeRuntime() && this.bridge.listDatabaseDatabases) {
      const password = await this.resolvePassword(connection.id)
      return this.bridge.listDatabaseDatabases(password ? { ...connection, password } : connection)
    }
    return session?.listDatabases() ?? []
  }
  async listTables(connection: DatabaseConnection, database: string): Promise<TableInfo[]> {
    const session = this.sessions.get(connection.id)
    if (this.hasNativeRuntime()) {
      const password = await this.resolvePassword(connection.id)
      const config: DatabaseConnectionConfig = password ? { ...connection, database, password } : { ...connection, database }
      if (connection.type === 'mysql' && this.bridge.queryDatabase) {
        const result = await this.bridge.queryDatabase(config, `SHOW FULL TABLES FROM ${quoteMysqlIdentifier(database)};`)
        return tablesFromShowResult(result)
      }
      if (this.bridge.listDatabaseTables) {
        const rawTables = await this.bridge.listDatabaseTables(config, database) as Array<TableInfo & { tableType?: string }>
        return rawTables.map((table) => ({ ...table, type: table.type ?? (table.tableType?.toLocaleLowerCase().includes('view') ? 'view' as const : 'table' as const) }))
      }
    }
    return session?.listTables(database) ?? []
  }
  async query(connection: DatabaseConnection, sql: string, limit: number, database?: string): Promise<QueryResult> {
    const preparedSql = this.seedQuery(sql, limit)
    if (this.hasNativeRuntime() && this.bridge.queryDatabase) {
      const password = await this.resolvePassword(connection.id)
      const selectedDatabase = database?.trim() || connection.database?.trim() || undefined
      const config: DatabaseConnectionConfig = password ? { ...connection, database: selectedDatabase, password } : { ...connection, database: selectedDatabase }
      return this.bridge.queryDatabase(config, preparedSql)
    }
    const session = this.sessions.get(connection.id) ?? await this.connect(connection)
    return session.query(preparedSql, { limit })
  }
  async connect(connection: DatabaseConnection): Promise<DatabaseSession> { const session = new PreviewDatabaseSession(connection); this.sessions.set(connection.id, session); return session }
  disconnect(id: string): void { this.sessions.delete(id) }
  getSession(id: string): DatabaseSession | undefined { return this.sessions.get(id) }
  listHistory(): DatabaseHistoryEntry[] { return readJson<DatabaseHistoryEntry[]>(HISTORY_KEY, []) }
  addHistory(entry: Omit<DatabaseHistoryEntry, 'id' | 'executedAt'>): void { writeJson(HISTORY_KEY, [{ ...entry, id: createId('query'), executedAt: Date.now() }, ...this.listHistory()].slice(0, MAX_HISTORY)) }
  clearHistory(): void { writeJson(HISTORY_KEY, []) }
  seedQuery(sql: string, limit: number): string { return applyResultLimit(sql, limit) }
  warningsFor(sql: string) { return analyzeSqlSafety(sql) }
  splitScript(sql: string): string[] { return splitSqlStatements(sql) }
  connectionTypeLabel(connection: DatabaseConnection): string { return databaseLabel(connection.type) }
  private hasNativeRuntime(): boolean { return typeof window !== 'undefined' && Reflect.has(window, '__TAURI_INTERNALS__') }
}

export const databaseWorkbench = new DatabaseWorkbenchService()
