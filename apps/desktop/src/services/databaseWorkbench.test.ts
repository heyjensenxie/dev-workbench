import { afterEach, describe, expect, it, vi } from 'vitest'
import type { DatabaseConnection, DatabaseConnectionConfig, QueryResult, TableInfo } from '@dev-workbench/shared'
import { DATABASE_MESSAGE_CODES } from '@dev-workbench/shared'
import { DatabaseWorkbenchService } from './databaseWorkbench'

const connection: DatabaseConnection = {
  id: 'connection-1',
  name: 'localhost',
  type: 'mysql',
  host: 'localhost',
  port: 3306,
  username: 'root',
  database: '',
  createdAt: 1,
  updatedAt: 1,
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('DatabaseWorkbenchService selected database', () => {
  it('uses the selected database when loading tables', async () => {
    vi.stubGlobal('window', { __TAURI_INTERNALS__: {} })
    const received: DatabaseConnectionConfig[] = []
    const service = new DatabaseWorkbenchService({
      listDatabaseTables: async (config): Promise<TableInfo[]> => {
        received.push(config)
        return []
      },
    })

    await service.listTables(connection, 'olinker')

    expect(received[0]?.database).toBe('olinker')
  })

  it('uses the proven SHOW query path to build the MySQL table list', async () => {
    vi.stubGlobal('window', { __TAURI_INTERNALS__: {} })
    const service = new DatabaseWorkbenchService({
      listDatabaseTables: async () => [],
      queryDatabase: async () => ({
        columns: [
          { name: 'Tables_in_olinker', type: 'TEXT' },
          { name: 'Table_type', type: 'TEXT' },
        ],
        rows: [
          { Tables_in_olinker: 'sys_user', Table_type: 'BASE TABLE' },
          { Tables_in_olinker: 'user_summary', Table_type: 'VIEW' },
        ],
        executionTime: 1,
      }),
    })

    await expect(service.listTables(connection, 'olinker')).resolves.toEqual([
      { name: 'sys_user', type: 'table' },
      { name: 'user_summary', type: 'view' },
    ])
  })

  it('uses the SHOW query path directly for MySQL without calling the unstable table IPC command', async () => {
    vi.stubGlobal('window', { __TAURI_INTERNALS__: {} })
    const listDatabaseTables = vi.fn(async (): Promise<TableInfo[]> => { throw new TypeError('Failed to fetch') })
    const service = new DatabaseWorkbenchService({
      listDatabaseTables,
      queryDatabase: async () => ({
        columns: [
          { name: 'Tables_in_olinker', type: 'TEXT' },
          { name: 'Table_type', type: 'TEXT' },
        ],
        rows: [{ Tables_in_olinker: 'sys_user', Table_type: 'BASE TABLE' }],
        executionTime: 1,
      }),
    })

    await expect(service.listTables(connection, 'olinker')).resolves.toEqual([
      { name: 'sys_user', type: 'table' },
    ])
    expect(listDatabaseTables).not.toHaveBeenCalled()
  })

  it('uses the selected database when executing SQL', async () => {
    vi.stubGlobal('window', { __TAURI_INTERNALS__: {} })
    const received: DatabaseConnectionConfig[] = []
    const result: QueryResult = { columns: [], rows: [], executionTime: 1 }
    const service = new DatabaseWorkbenchService({
      queryDatabase: async (config) => {
        received.push(config)
        return result
      },
    })

    await service.query(connection, 'SELECT * FROM sys_user', 100, 'olinker')

    expect(received[0]?.database).toBe('olinker')
  })
})

describe('DatabaseWorkbenchService preview runtime messages', () => {
  it('describes its own results with locale-neutral codes', async () => {
    vi.stubGlobal('window', { setTimeout: globalThis.setTimeout })
    const service = new DatabaseWorkbenchService({})

    const connectionResult = await service.testConnection({ id: 'connection-1', name: 'local', type: 'sqlite' })
    expect(connectionResult.message).toBe(DATABASE_MESSAGE_CODES.connectedSqlite)

    const session = await service.connect(connection)
    const result = await session.query('SELECT * FROM projects', { limit: 1 })
    expect(result.truncated).toBe(true)
    expect(result.message).toBe(DATABASE_MESSAGE_CODES.limitReached)
  })
})
