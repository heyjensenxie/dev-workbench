<script setup lang="ts">
import type { ColumnInfo, ConnectionTestResult, DatabaseConnection, DatabaseConnectionConfig, DatabaseHistoryEntry, DatabaseInfo, DatabaseSession, ExplainResult, IndexInfo, QueryResult, TableInfo } from '@dev-workbench/shared'
import { databaseLabel, inferColumnType, isReadQuery } from '@dev-workbench/core'
import { Check, ChevronDown, ChevronRight, CircleAlert, Clipboard, Code2, Columns3, Copy, Database, FileCode2, History, KeyRound, Link2, List, LoaderCircle, MoreHorizontal, Play, Plus, RefreshCw, Search, Server, ShieldAlert, Table2, Terminal, Trash2, Unplug, WandSparkles, WrapText, X, Zap } from 'lucide-vue-next'
import { computed, nextTick, onMounted, reactive, ref } from 'vue'
import { translateDatabaseMessage, translateSqlWarning, useI18n, type MessageKey, type MessageParams } from '../i18n'
import { databaseWorkbench } from '../services/databaseWorkbench'
import { workbench } from '../services/workbench'

interface QueryTab { id: string; number: number; sql: string; dirty: boolean }
type ResultView = 'result' | 'messages' | 'explain' | 'structure' | 'ddl' | 'history'
/** Execution feedback kept as a key so it follows the active interface language. */
type ResultMessage =
  | { tone: 'info' | 'error'; key: MessageKey; params?: MessageParams }
  | { tone: 'info' | 'error'; text: string }

const { t } = useI18n()

const connections = ref<DatabaseConnection[]>([])
const connectionStatuses = reactive<Record<string, 'disconnected' | 'connecting' | 'connected' | 'failed'>>({})
const connectionErrors = reactive<Record<string, string>>({})
const activeConnectionId = ref<string>()
const activeDatabase = ref<string>()
const activeTable = ref<string>()
const sessions = new Map<string, DatabaseSession>()
const databases = ref<Record<string, DatabaseInfo[]>>({})
const tables = ref<Record<string, TableInfo[]>>({})
const tableErrors = reactive<Record<string, string>>({})
const expandedConnections = ref(new Set<string>())
const expandedDatabases = ref(new Set<string>())
const expandedTables = ref(new Set<string>())
const loadedColumns = ref<Record<string, ColumnInfo[]>>({})
const loadedIndexes = ref<Record<string, IndexInfo[]>>({})
const ddlByTable = ref<Record<string, string>>({})
const queryTabs = ref<QueryTab[]>([{ id: 'query-1', number: 1, sql: '', dirty: false }])
const activeTabId = ref('query-1')
const query = computed({ get: () => queryTabs.value.find((tab) => tab.id === activeTabId.value)?.sql ?? '', set: (sql: string) => { const tab = queryTabs.value.find((item) => item.id === activeTabId.value); if (tab) { tab.sql = sql; tab.dirty = true } } })
const resultView = ref<ResultView>('result')
const result = ref<QueryResult>()
const explainResult = ref<ExplainResult>()
const messages = ref<ResultMessage[]>([])
const isRunning = ref(false)
const resultLimit = ref(500)
const queryTimeout = ref(30000)
const search = ref('')
const wrapCells = ref(initialWrapCells())
const historySearch = ref('')
const history = ref<DatabaseHistoryEntry[]>([])
const showHistory = ref(false)
const toast = ref<string>()
const copied = ref<string>()
const pendingWarnings = ref<{ sql: string; warnings: ReturnType<typeof databaseWorkbench.warningsFor> }>()
const transactionActive = ref(false)
const showConnectionDialog = ref(false)
const testingConnection = ref(false)
const rememberPassword = ref(true)
const testResult = ref<ConnectionTestResult>()
const draft = reactive<DatabaseConnectionConfig>({ id: '', name: '', type: 'mysql', host: 'localhost', port: 3306, username: 'root', database: '', sqlitePath: '', password: '', options: {} })

const activeConnection = computed(() => connections.value.find((connection) => connection.id === activeConnectionId.value))
const activeStatus = computed(() => activeConnection.value ? connectionStatuses[activeConnection.value.id] ?? 'disconnected' : 'disconnected')
const currentTab = computed(() => queryTabs.value.find((tab) => tab.id === activeTabId.value))
const filteredHistory = computed(() => history.value.filter((entry) => entry.sqlText.toLocaleLowerCase().includes(historySearch.value.toLocaleLowerCase())))
const currentColumns = computed(() => activeConnection.value && activeTable.value ? loadedColumns.value[`${activeConnection.value.id}:${activeTable.value}`] ?? [] : [])
const currentIndexes = computed(() => activeConnection.value && activeTable.value ? loadedIndexes.value[`${activeConnection.value.id}:${activeTable.value}`] ?? [] : [])

function statusLabel(status: string): string {
  if (status === 'connected') return t('dbStatusConnected')
  if (status === 'connecting') return t('dbStatusConnecting')
  if (status === 'failed') return t('dbStatusFailed')
  return t('dbStatusDisconnected')
}
function statusTone(status: string): string { return status === 'connected' ? 'is-connected' : status === 'failed' ? 'is-failed' : status === 'connecting' ? 'is-connecting' : '' }
function tabName(tab: QueryTab): string { return t('dbQueryTabName', { number: tab.number }) }
function messageText(message: ResultMessage): string { return 'key' in message ? t(message.key, message.params) : message.text }
function flash(message: string): void { toast.value = message; window.setTimeout(() => { if (toast.value === message) toast.value = undefined }, 2200) }
function errorDetails(error: unknown, fallback: string): { code: string; message: string } {
  if (typeof error === 'string') return { code: 'DB_ERROR', message: error }
  const payload = error as { code?: unknown; message?: unknown } | undefined
  return { code: typeof payload?.code === 'string' ? payload.code : 'DB_ERROR', message: typeof payload?.message === 'string' ? payload.message : fallback }
}

/** Surfaces a database failure with its code, keeping raw engine wording intact. */
function showError(code: string, message: string): void {
  messages.value = [
    { tone: 'error', key: 'dbMsgErrorCode', params: { code } },
    { tone: 'error', text: message },
  ]
  resultView.value = 'messages'
}
/** Cells wrap long values instead of clipping them at the column cap. */
function initialWrapCells(): boolean {
  try { return typeof localStorage !== 'undefined' && localStorage.getItem('database-wrap-cells') === 'true' } catch { return false }
}
function toggleWrapCells(): void {
  wrapCells.value = !wrapCells.value
  try { localStorage.setItem('database-wrap-cells', String(wrapCells.value)) } catch { /* Optional UI preference. */ }
}
function visibleTablesFor(connectionId: string, databaseName: string): TableInfo[] {
  const term = search.value.toLocaleLowerCase()
  return (tables.value[`${connectionId}:${databaseName}`] ?? []).filter((table) => table.name.toLocaleLowerCase().includes(term))
}
function makeId(): string { return `connection-${globalThis.crypto?.randomUUID?.() ?? Date.now().toString(36)}` }

function openNewConnection(): void {
  Object.assign(draft, { id: makeId(), name: '', type: 'mysql', host: 'localhost', port: 3306, username: 'root', database: '', sqlitePath: '', password: '', options: {} }); rememberPassword.value = true
  testResult.value = undefined
  showConnectionDialog.value = true
}
function editConnection(connection: DatabaseConnection): void { Object.assign(draft, connection, { password: '' }); rememberPassword.value = Boolean(connection.secretRef); testResult.value = undefined; showConnectionDialog.value = true }
function closeDialog(): void { showConnectionDialog.value = false }
async function testDraft(): Promise<void> { testingConnection.value = true; testResult.value = undefined; try { testResult.value = await databaseWorkbench.testConnection(draft) } finally { testingConnection.value = false } }
async function saveDraft(): Promise<void> {
  if (!draft.name.trim()) { testResult.value = { success: false, message: t('dbConnectionNameRequired') }; return }
  const saved = databaseWorkbench.saveConnection(draft, rememberPassword.value)
  try { await databaseWorkbench.persistPassword(saved.id, draft.password ?? '', rememberPassword.value) } catch (error) { testResult.value = { success: false, message: error instanceof Error ? error.message : t('dbPasswordStoreFailed') }; return }
  connections.value = databaseWorkbench.listConnections()
  showConnectionDialog.value = false
  await connect(saved)
}
function deleteConnection(connection: DatabaseConnection): void {
  if (!window.confirm(t('dbRemoveConnection', { name: connection.name }))) return
  databaseWorkbench.removeConnection(connection.id)
  connections.value = databaseWorkbench.listConnections()
  if (activeConnectionId.value === connection.id) { activeConnectionId.value = undefined; activeDatabase.value = undefined; activeTable.value = undefined }
}

async function selectConnection(connection: DatabaseConnection): Promise<void> { activeConnectionId.value = connection.id; expandedConnections.value.add(connection.id); workbench.context.setDatabase({ connectionId: connection.id, type: connection.type, database: connection.database }); if (connectionStatuses[connection.id] !== 'connected') await connect(connection) }
async function connect(connection: DatabaseConnection): Promise<void> {
  activeConnectionId.value = connection.id; connectionStatuses[connection.id] = 'connecting'; delete connectionErrors[connection.id]
  try {
    const session = await databaseWorkbench.connect(connection)
    sessions.set(connection.id, session)
    connectionStatuses[connection.id] = 'connected'
    expandedConnections.value.add(connection.id)
    await loadDatabases(connection)
    workbench.context.setDatabase({ connectionId: connection.id, type: connection.type, database: connection.database })
    flash(t('dbToastConnected', { name: connection.name }))
  } catch (error) { const message = error instanceof Error ? error.message : t('dbUnableToConnect'); connectionStatuses[connection.id] = 'failed'; connectionErrors[connection.id] = message; showError('DB_ERROR', message) }
}
function disconnect(connection: DatabaseConnection): void { databaseWorkbench.disconnect(connection.id); sessions.delete(connection.id); connectionStatuses[connection.id] = 'disconnected'; expandedConnections.value.delete(connection.id); flash(t('dbToastDisconnected', { name: connection.name })) }
async function loadDatabases(connection: DatabaseConnection): Promise<void> { const session = sessions.get(connection.id); if (!session) return; try { databases.value[connection.id] = await databaseWorkbench.listDatabases(connection); delete connectionErrors[connection.id] } catch (error) { const message = error instanceof Error ? error.message : t('dbUnableToListDatabases'); connectionStatuses[connection.id] = 'failed'; connectionErrors[connection.id] = message; showError('DB_ERROR', message) } }
async function toggleConnection(connection: DatabaseConnection): Promise<void> { if (expandedConnections.value.has(connection.id)) expandedConnections.value.delete(connection.id); else { expandedConnections.value.add(connection.id); if (connectionStatuses[connection.id] !== 'connected') await connect(connection) } }
async function toggleDatabase(connection: DatabaseConnection, database: DatabaseInfo): Promise<void> {
  const key = `${connection.id}:${database.name}`
  activeConnectionId.value = connection.id; activeDatabase.value = database.name; workbench.context.setDatabase({ connectionId: connection.id, type: connection.type, database: database.name, activeQuery: query.value })
  if (expandedDatabases.value.has(key)) { expandedDatabases.value.delete(key); return }
  expandedDatabases.value.add(key)
  try {
    const session = sessions.get(connection.id)
    if (session) tables.value[key] = await databaseWorkbench.listTables(connection, database.name)
    delete tableErrors[key]
  } catch (error) {
    const details = errorDetails(error, t('dbUnableToListTables'))
    tableErrors[key] = details.message
    showError(details.code, details.message)
  }
}
async function selectDatabaseFromToolbar(databaseName: string): Promise<void> {
  const connection = activeConnection.value
  if (!connection) return
  const database = databases.value[connection.id]?.find((item) => item.name === databaseName)
  if (!database) { activeDatabase.value = databaseName || undefined; return }
  const key = `${connection.id}:${database.name}`
  if (expandedDatabases.value.has(key)) await reloadTables(connection, database)
  else await toggleDatabase(connection, database)
}
async function reloadTables(connection: DatabaseConnection, database: DatabaseInfo): Promise<void> {
  const key = `${connection.id}:${database.name}`
  delete tables.value[key]
  expandedDatabases.value.delete(key)
  await toggleDatabase(connection, database)
}
async function selectTable(connection: DatabaseConnection, table: TableInfo): Promise<void> {
  const key = `${connection.id}:${table.name}`
  activeConnectionId.value = connection.id; activeTable.value = table.name; workbench.context.setDatabase({ connectionId: connection.id, type: connection.type, database: activeDatabase.value, selectedTable: table.name, activeQuery: query.value })
  if (expandedTables.value.has(key)) return
  expandedTables.value.add(key)
  const session = sessions.get(connection.id); if (!session) return
  loadedColumns.value[key] ??= await session.getTableColumns({ database: activeDatabase.value, table: table.name })
  loadedIndexes.value[key] ??= await session.getIndexes({ database: activeDatabase.value, table: table.name })
}
async function toggleTable(connection: DatabaseConnection, table: TableInfo): Promise<void> { const key = `${connection.id}:${table.name}`; if (expandedTables.value.has(key)) { expandedTables.value.delete(key); return } await selectTable(connection, table) }
function generateSelect(table = activeTable.value): void { if (!table) return; query.value = `SELECT *\nFROM ${activeDatabase.value ? `${activeDatabase.value}.` : ''}${table}\nLIMIT 100;`; resultView.value = 'result'; flash(t('dbToastSelectGenerated')) }
async function openTable(connection: DatabaseConnection, table: TableInfo): Promise<void> { await selectTable(connection, table); generateSelect(table.name); await nextTick(); await runQuery() }
async function openStructure(table: TableInfo): Promise<void> { activeTable.value = table.name; resultView.value = 'structure'; if (activeConnection.value) await selectTable(activeConnection.value, table) }
async function showDdl(table: TableInfo): Promise<void> { activeTable.value = table.name; const session = activeConnection.value && sessions.get(activeConnection.value.id); if (session) ddlByTable.value[`${activeConnection.value.id}:${table.name}`] = await session.getCreateTable({ database: activeDatabase.value, table: table.name }); resultView.value = 'ddl' }

async function runQuery(overrideSql?: string, force = false, wholeScript = false): Promise<void> {
  const connection = activeConnection.value; const session = connection && sessions.get(connection.id); const sql = (overrideSql ?? query.value).trim()
  if (!connection || !session) { messages.value = [{ tone: 'error', key: 'dbMsgConnectFirst' }]; resultView.value = 'messages'; return }
  if (!sql) return
  const warnings = databaseWorkbench.warningsFor(sql)
  if (warnings.length && !force) { pendingWarnings.value = { sql, warnings }; return }
  isRunning.value = true; messages.value = []; resultView.value = 'result'; result.value = undefined; workbench.context.setDatabase({ connectionId: connection.id, type: connection.type, database: activeDatabase.value, selectedTable: activeTable.value, activeQuery: sql })
  const started = performance.now()
  try {
    const statements = wholeScript ? databaseWorkbench.splitScript(sql) : [databaseWorkbench.seedQuery(sql, resultLimit.value)]
    let lastResult: QueryResult | undefined
    for (const statement of statements) lastResult = await databaseWorkbench.query(connection, statement, resultLimit.value, activeDatabase.value)
    result.value = lastResult
    const notice = translateDatabaseMessage(result.value?.message)
    messages.value = [
      statements.length === 1
        ? { tone: 'info', key: 'dbMsgStatementExecuted' }
        : { tone: 'info', key: 'dbMsgStatementsExecuted', params: { count: statements.length } },
      ...(notice ? [{ tone: 'info' as const, text: notice }] : []),
    ]
    databaseWorkbench.addHistory({ connectionId: connection.id, database: activeDatabase.value, sqlText: sql, success: true, executionTime: Math.round(performance.now() - started) })
    history.value = databaseWorkbench.listHistory()
  } catch (error) {
    const details = errorDetails(error, t('dbDatabaseError'))
    showError(details.code, details.message)
    databaseWorkbench.addHistory({ connectionId: connection.id, database: activeDatabase.value, sqlText: sql, success: false, errorCode: details.code, executionTime: Math.round(performance.now() - started) })
    history.value = databaseWorkbench.listHistory()
  } finally { isRunning.value = false }
}
async function cancelQuery(): Promise<void> { const session = activeConnection.value && sessions.get(activeConnection.value.id); await session?.cancel?.(); isRunning.value = false; messages.value = [{ tone: 'info', key: 'dbMsgQueryCancelled' }]; resultView.value = 'messages' }
async function runExplain(): Promise<void> { const session = activeConnection.value && sessions.get(activeConnection.value.id); if (!session?.explain) return; isRunning.value = true; try { explainResult.value = await session.explain(query.value); resultView.value = 'explain' } finally { isRunning.value = false } }
function formatSql(): void { query.value = query.value.replace(/\s+/g, ' ').replace(/\s*,\s*/g, ', ').replace(/\b(from|where|join|group by|order by|limit|left join|inner join)\b/gi, '\n$1').trim(); flash(t('dbToastSqlFormatted')) }
function addQueryTab(): void { const number = queryTabs.value.reduce((highest, tab) => Math.max(highest, tab.number), 0) + 1; const tab: QueryTab = { id: `query-${Date.now()}`, number, sql: '', dirty: false }; queryTabs.value.push(tab); activeTabId.value = tab.id }
function closeQueryTab(tab: QueryTab): void { if (tab.dirty && !window.confirm(t('dbDiscardChanges', { name: tabName(tab) }))) return; if (queryTabs.value.length === 1) return; const index = queryTabs.value.indexOf(tab); queryTabs.value = queryTabs.value.filter((item) => item.id !== tab.id); if (tab.id === activeTabId.value) activeTabId.value = queryTabs.value[Math.max(0, index - 1)]?.id ?? queryTabs.value[0]!.id }
function executeCurrentQuery(): void { void runQuery() }
function executeCurrentScript(): void { void runQuery(undefined, false, true) }
function confirmPendingQuery(): void { const sql = pendingWarnings.value?.sql; pendingWarnings.value = undefined; if (sql) void runQuery(sql, true) }
function handleEditorKeydown(event: KeyboardEvent): void { if ((event.ctrlKey || event.metaKey) && event.key === 'Enter') { event.preventDefault(); if (event.shiftKey) executeCurrentScript(); else executeCurrentQuery() } }
function valueText(value: unknown): string { if (value === null || value === undefined) return 'NULL'; if (typeof value === 'object') return JSON.stringify(value); return String(value) }
function valueClass(value: unknown): string { return `value-${inferColumnType(value).toLocaleLowerCase()}` }
async function copyText(text: string, label = t('dbToastCopied')): Promise<void> { try { await navigator.clipboard.writeText(text); copied.value = text; flash(label); window.setTimeout(() => { copied.value = undefined }, 1200) } catch { flash(t('dbClipboardUnavailable')) } }
function copyRow(row: Record<string, unknown>): void { void copyText(JSON.stringify(row, null, 2), t('dbToastRowCopied')) }
function copyCsv(): void { if (!result.value) return; const header = result.value.columns.map((column) => column.name).join(','); const lines = result.value.rows.map((row) => result.value!.columns.map((column) => JSON.stringify(row[column.name] ?? '')).join(',')); void copyText([header, ...lines].join('\n'), t('dbToastCsvCopied')) }
function copyInsert(): void { if (!result.value) return; const table = activeTable.value ?? 'table_name'; const columns = result.value.columns.map((column) => column.name).join(', '); const lines = result.value.rows.map((row) => `INSERT INTO ${table} (${columns}) VALUES (${result.value!.columns.map((column) => row[column.name] === null ? 'NULL' : JSON.stringify(row[column.name])).join(', ')});`); void copyText(lines.join('\n'), t('dbToastInsertCopied')) }
function runHistoryEntry(entry: DatabaseHistoryEntry): void { query.value = entry.sqlText; activeConnectionId.value = entry.connectionId; activeDatabase.value = entry.database; showHistory.value = false; resultView.value = 'result'; void runQuery() }
function clearHistory(): void { databaseWorkbench.clearHistory(); history.value = [] }
function beginTransaction(): void { transactionActive.value = true; messages.value = [{ tone: 'info', key: 'dbMsgTransactionStarted' }]; flash(t('dbTransactionActive')) }
function finishTransaction(action: 'commit' | 'rollback'): void {
  transactionActive.value = false
  const key: MessageKey = action === 'commit' ? 'dbMsgTransactionCommitted' : 'dbMsgTransactionRolledBack'
  messages.value = [{ tone: 'info', key }]
  flash(t(key))
}

onMounted(() => { connections.value = databaseWorkbench.listConnections(); history.value = databaseWorkbench.listHistory() })
</script>

<template>
  <section class="view database-view">
    <header class="view-header database-header">
      <div><span class="section-kicker">{{ t('dbKicker') }}</span><h1>{{ t('dbTitle') }}</h1><p>{{ t('dbSubtitle') }}</p></div>
      <div class="database-header-actions"><span class="local-badge"><span></span>{{ t('dbLocalFirst') }}</span><button class="primary" @click="openNewConnection"><Plus :size="14" /> {{ t('dbNewConnection') }}</button></div>
    </header>
    <div v-if="transactionActive" class="transaction-banner"><Zap :size="14" /><strong>{{ t('dbTransactionActive') }}</strong><span>{{ t('dbTransactionActiveHint') }}</span><button @click="finishTransaction('rollback')">{{ t('dbRollback') }}</button><button @click="finishTransaction('commit')">{{ t('dbCommit') }}</button></div>
    <div class="database-layout">
      <aside class="database-explorer">
        <div class="explorer-heading"><span><Database :size="14" /> {{ t('dbConnections') }}</span><button class="icon-button" :aria-label="t('dbNewConnection')" :title="t('dbNewConnection')" @click="openNewConnection"><Plus :size="14" /></button></div>
        <div v-if="!connections.length" class="explorer-empty"><div class="empty-mark"><Database :size="17" /></div><strong>{{ t('dbNoConnections') }}</strong><p>{{ t('dbNoConnectionsHint') }}</p><button class="ghost-button" @click="openNewConnection"><Plus :size="13" /> {{ t('dbNewConnection') }}</button></div>
        <div v-else class="connection-list">
          <div v-for="connection in connections" :key="connection.id" class="connection-block">
            <div class="connection-row" :class="{ active: activeConnectionId === connection.id }" role="button" tabindex="0" @click="selectConnection(connection)" @keydown.enter="selectConnection(connection)">
              <button class="tree-toggle" :aria-label="t('dbToggleConnection', { name: connection.name })" @click.stop="toggleConnection(connection)"><ChevronDown v-if="expandedConnections.has(connection.id)" :size="13" /><ChevronRight v-else :size="13" /></button><span class="db-type-icon" :class="connection.type"><Server v-if="connection.type === 'mysql'" :size="14" /><FileCode2 v-else :size="14" /></span><span class="connection-copy"><strong>{{ connection.name }}</strong><small>{{ databaseLabel(connection.type) }} · {{ connection.host ?? connection.sqlitePath ?? t('dbLocalFile') }}</small></span><span class="connection-status" :class="statusTone(connectionStatuses[connection.id] ?? 'disconnected')" :title="statusLabel(connectionStatuses[connection.id] ?? 'disconnected')"></span><button class="icon-button row-more" :aria-label="t('dbConnectionActions')" @click.stop="editConnection(connection)"><MoreHorizontal :size="13" /></button>
            </div>
            <div v-if="expandedConnections.has(connection.id)" class="tree-children">
              <div v-if="connectionStatuses[connection.id] !== 'connected'" class="tree-hint"><CircleAlert :size="12" /><span v-if="connectionErrors[connection.id]" class="tree-error">{{ connectionErrors[connection.id] }}</span><span v-else>{{ t('dbConnectToBrowse') }}</span><button v-if="connectionErrors[connection.id]" class="tree-retry" @click="editConnection(connection)">{{ t('dbEditCredentials') }}</button><button v-else class="tree-retry" @click="connect(connection)">{{ t('dbConnect') }}</button></div>
              <template v-for="database in databases[connection.id] ?? []" :key="database.name">
                <button class="tree-row database-row" :class="{ active: activeDatabase === database.name && activeConnectionId === connection.id }" @click="toggleDatabase(connection, database)"><ChevronDown v-if="expandedDatabases.has(`${connection.id}:${database.name}`)" :size="12" /><ChevronRight v-else :size="12" /><Database :size="13" /><span>{{ database.name }}</span></button>
                <div v-if="expandedDatabases.has(`${connection.id}:${database.name}`)" class="tree-children nested">
                  <div v-if="tableErrors[`${connection.id}:${database.name}`]" class="tree-hint"><CircleAlert :size="12" /><span class="tree-error">{{ tableErrors[`${connection.id}:${database.name}`] }}</span><button class="tree-retry" @click.stop="reloadTables(connection, database)">{{ t('dbRetry') }}</button></div>
                  <div class="tree-row collection-row"><Table2 :size="12" /><span>{{ t('dbTables') }}</span><small>{{ tables[`${connection.id}:${database.name}`]?.filter((item) => item.type === 'table').length ?? '…' }}</small></div>
                  <template v-for="table in visibleTablesFor(connection.id, database.name)" :key="table.name">
                    <button v-if="table.type === 'table'" class="tree-row table-row" :class="{ active: activeTable === table.name && activeConnectionId === connection.id }" @dblclick.stop="openTable(connection, table)" @click="selectTable(connection, table)"><ChevronDown v-if="expandedTables.has(`${connection.id}:${table.name}`)" :size="11" /><ChevronRight v-else :size="11" /><Table2 :size="12" /><span>{{ table.name }}</span><small v-if="table.rows !== undefined">{{ table.rows.toLocaleString() }}</small></button>
                    <div v-if="expandedTables.has(`${connection.id}:${table.name}`)" class="table-subtree"><button class="tree-row sub-row" @click="openStructure(table)"><Columns3 :size="11" /> {{ t('dbColumns') }} <small>{{ loadedColumns[`${connection.id}:${table.name}`]?.length ?? 0 }}</small></button><button class="tree-row sub-row" @click="showDdl(table)"><Code2 :size="11" /> {{ t('dbDdl') }}</button></div>
                  </template>
                  <div class="tree-row collection-row"><List :size="12" /><span>{{ t('dbViews') }}</span><small>{{ tables[`${connection.id}:${database.name}`]?.filter((item) => item.type === 'view').length ?? 0 }}</small></div>
                  <button v-for="view in visibleTablesFor(connection.id, database.name).filter((item) => item.type === 'view')" :key="view.name" class="tree-row table-row view-row" @dblclick.stop="activeConnection && openTable(activeConnection, view)" @click="activeTable = view.name"><ChevronRight :size="11" /><Table2 :size="12" /><span>{{ view.name }}</span></button>
                  <div v-if="tables[`${connection.id}:${database.name}`]?.length === 0 && !tableErrors[`${connection.id}:${database.name}`]" class="tree-hint"><span>{{ t('dbNoTables') }}</span></div>
                </div>
              </template>
            </div>
          </div>
        </div>
        <div class="explorer-footer"><button class="ghost-button" @click="showHistory = true; resultView = 'history'"><History :size="13" /> {{ t('dbSqlHistory') }} <span>{{ history.length }}</span></button><button class="ghost-button" @click="activeConnection && loadDatabases(activeConnection)"><RefreshCw :size="13" /> {{ t('refresh') }}</button></div>
      </aside>

      <main class="database-main">
        <div class="database-toolbar">
          <div class="context-selects"><label>{{ t('dbConnectionSelect') }} <select :value="activeConnectionId" @change="selectConnection(connections.find((item) => item.id === ($event.target as HTMLSelectElement).value)!)"><option value="" disabled>{{ t('dbSelectConnection') }}</option><option v-for="connection in connections" :key="connection.id" :value="connection.id">{{ connection.name }}</option></select></label><span class="slash">/</span><label>{{ t('database') }} <select :value="activeDatabase ?? ''" :disabled="!activeConnection" @change="selectDatabaseFromToolbar(($event.target as HTMLSelectElement).value)"><option value="">{{ t('dbSelectDatabase') }}</option><option v-for="database in activeConnection ? databases[activeConnection.id] ?? [] : []" :key="database.name" :value="database.name">{{ database.name }}</option></select></label></div>
          <div class="toolbar-actions"><button class="ghost-button" :disabled="!activeConnection || activeStatus === 'connecting'" @click="activeStatus === 'connected' ? disconnect(activeConnection!) : connect(activeConnection!)"><Unplug :size="13" /> {{ activeStatus === 'connected' ? t('dbDisconnect') : t('dbConnect') }}</button><button class="ghost-button" @click="showHistory = true; resultView = 'history'"><History :size="13" /> {{ t('dbHistory') }}</button></div>
        </div>
        <div class="query-tabs" role="tablist"><button v-for="tab in queryTabs" :key="tab.id" :class="{ active: tab.id === activeTabId }" role="tab" @click="activeTabId = tab.id"><FileCode2 :size="12" />{{ tabName(tab) }}<span v-if="tab.dirty">•</span><X v-if="queryTabs.length > 1" :size="11" class="tab-close" :aria-label="t('dbCloseTab')" @click.stop="closeQueryTab(tab)" /></button><button class="new-tab" :aria-label="t('dbNewTab')" @click="addQueryTab"><Plus :size="13" /></button></div>
        <section class="sql-editor-panel">
          <div class="editor-toolbar"><div class="editor-toolbar-left"><span class="dialect-chip">{{ activeConnection ? databaseLabel(activeConnection.type) : 'SQL' }}</span><span class="editor-state"><span class="status-dot" :class="{ 'is-active': isRunning }"></span>{{ isRunning ? t('dbRunning') : activeConnection ? statusLabel(activeStatus) : t('dbNoConnection') }}</span></div><div class="editor-toolbar-right"><label class="compact-control">{{ t('timeout') }} <input v-model.number="queryTimeout" type="number" min="1000" step="1000" /> ms</label><button class="icon-button" :title="t('dbFormatSql')" :aria-label="t('dbFormatSql')" @click="formatSql"><WandSparkles :size="14" /></button><button v-if="isRunning" class="danger-outline" @click="cancelQuery"><X :size="13" /> {{ t('cancel') }}</button><button v-else class="primary run-button" @click="executeCurrentQuery"><Play :size="13" fill="currentColor" /> {{ t('dbRun') }} <kbd>Ctrl ↵</kbd></button><button class="ghost-button" :disabled="!activeConnection || isRunning" @click="runExplain"><Zap :size="13" /> {{ t('dbExplain') }}</button></div></div>
          <div class="editor-wrap"><div class="line-numbers">1<br>2<br>3<br>4<br>5<br>6<br>7<br>8<br>9</div><textarea v-model="query" class="sql-editor" spellcheck="false" :aria-label="t('dbEditor')" @keydown="handleEditorKeydown" /></div>
          <div class="editor-footer"><span><span class="key-hint">Ctrl / ⌘ + Enter</span> {{ t('dbRunSelectionHint') }} · <span class="key-hint">Ctrl / ⌘ + Shift + Enter</span> {{ t('dbRunScriptHint') }}</span><div class="transaction-actions"><button class="ghost-button" :disabled="transactionActive || !activeConnection" @click="beginTransaction">{{ t('dbBeginTransaction') }}</button><button class="ghost-button" :disabled="!transactionActive" @click="finishTransaction('rollback')">{{ t('dbRollback') }}</button></div></div>
        </section>

        <section class="result-panel">
          <div class="result-header"><div class="result-tabs"><button :class="{ active: resultView === 'result' }" @click="resultView = 'result'">{{ t('dbResult') }} <span v-if="result">{{ result.rows.length }}</span></button><button :class="{ active: resultView === 'messages' }" @click="resultView = 'messages'">{{ t('dbMessages') }} <span v-if="messages.length">{{ messages.length }}</span></button><button :class="{ active: resultView === 'explain' }" @click="resultView = 'explain'">{{ t('dbExplain') }}</button><button v-if="activeTable" :class="{ active: resultView === 'structure' }" @click="resultView = 'structure'">{{ t('dbStructure') }}</button><button v-if="activeTable" :class="{ active: resultView === 'ddl' }" @click="resultView = 'ddl'">{{ t('dbDdl') }}</button><button :class="{ active: resultView === 'history' }" @click="resultView = 'history'">{{ t('dbHistory') }}</button></div><div class="result-actions"><label class="compact-control">{{ t('dbLimit') }} <select v-model.number="resultLimit"><option :value="100">100</option><option :value="500">500</option><option :value="1000">1,000</option><option :value="5000">5,000</option></select></label><button class="icon-button" :title="t('dbCopyCsv')" :aria-label="t('dbCopyCsv')" :disabled="!result" @click="copyCsv"><Clipboard :size="14" /></button><button class="icon-button" :title="t('dbCopyInsert')" :aria-label="t('dbCopyInsert')" :disabled="!result" @click="copyInsert"><Code2 :size="14" /></button></div></div>
          <div v-if="resultView === 'result'" class="result-content"><div v-if="result" class="grid-frame"><div class="grid-scroll"><table class="data-grid" :class="{ 'is-wrapped': wrapCells }"><thead><tr><th class="row-number">#</th><th v-for="column in result.columns" :key="column.name"><span>{{ column.name }}</span><small>{{ column.type }}</small></th><th class="grid-spacer"></th></tr></thead><tbody><tr v-for="(row, rowIndex) in result.rows" :key="rowIndex"><td class="row-number">{{ rowIndex + 1 }}</td><td v-for="column in result.columns" :key="column.name" :class="valueClass(row[column.name])" :title="valueText(row[column.name])" @dblclick="copyText(valueText(row[column.name]), t('dbToastCellCopied'))"><span>{{ valueText(row[column.name]) }}</span></td><td class="row-actions"><button class="icon-button" :aria-label="t('dbCopyRow')" @click="copyRow(row)"><Copy :size="12" /></button></td></tr></tbody></table></div><div class="grid-footer"><span v-if="result.truncated" class="truncated-note"><CircleAlert :size="12" /> {{ t('dbShowingFirstRows', { count: result.rows.length }) }}</span><span v-else>{{ t('dbRowSummary', { rows: result.rows.length, ms: result.executionTime }) }}</span><button class="ghost-button" :class="{ active: wrapCells }" :aria-pressed="wrapCells" :title="t('dbWrapCells')" @click="toggleWrapCells"><WrapText :size="12" /> {{ t('dbWrapCells') }}</button><button class="ghost-button" @click="copyText(JSON.stringify(result.rows, null, 2), t('dbToastJsonCopied'))">{{ t('dbCopyAsJson') }}</button></div></div><div v-else class="result-empty"><div class="empty-mark"><Terminal :size="17" /></div><strong>{{ t('dbResultEmptyTitle') }}</strong><p>{{ t('dbResultEmptyHint') }}</p></div></div>
          <div v-else-if="resultView === 'messages'" class="messages-view"><div v-if="!messages.length" class="result-empty compact"><strong>{{ t('dbMessagesEmptyTitle') }}</strong><p>{{ t('dbMessagesEmptyHint') }}</p></div><div v-for="(message, index) in messages" :key="index" class="message-row" :class="{ error: message.tone === 'error' }"><CircleAlert v-if="message.tone === 'error'" :size="14" /><Check v-else :size="14" /><span>{{ messageText(message) }}</span></div></div>
          <div v-else-if="resultView === 'explain'" class="explain-view"><div v-if="explainResult" class="grid-frame"><div class="grid-scroll"><table class="data-grid explain-grid"><thead><tr><th v-for="column in explainResult.columns" :key="column.name">{{ column.name }}</th></tr></thead><tbody><tr v-for="(row, index) in explainResult.rows" :key="index"><td v-for="column in explainResult.columns" :key="column.name" :class="valueClass(row[column.name])">{{ valueText(row[column.name]) }}</td></tr></tbody></table></div><div class="explain-insights"><span><Check :size="12" /> {{ t('dbIndexConditionUsed') }}</span><span><Zap :size="12" /> {{ t('dbPlanningTime', { ms: explainResult.executionTime }) }}</span></div></div><div v-else class="result-empty compact"><strong>{{ t('dbExplainEmptyTitle') }}</strong><p>{{ t('dbExplainEmptyHint') }}</p></div></div>
          <div v-else-if="resultView === 'structure'" class="structure-view"><div v-if="currentColumns.length" class="structure-section"><div class="subsection-heading"><span><Columns3 :size="13" /> {{ t('dbColumns') }}</span><small>{{ currentColumns.length }}</small></div><table class="structure-table"><thead><tr><th>{{ t('dbColumnName') }}</th><th>{{ t('dbColumnType') }}</th><th>{{ t('dbNullable') }}</th><th>{{ t('dbDefault') }}</th><th>{{ t('dbKey') }}</th><th>{{ t('dbComment') }}</th></tr></thead><tbody><tr v-for="column in currentColumns" :key="column.name"><td><KeyRound v-if="column.primaryKey" :size="12" class="key-icon" />{{ column.name }}</td><td class="mono type-cell">{{ column.type }}</td><td>{{ column.nullable ? t('dbYes') : t('dbNo') }}</td><td class="mono muted-cell">{{ column.defaultValue ?? '—' }}</td><td>{{ column.primaryKey ? 'PRIMARY' : '—' }}</td><td class="muted-cell">{{ column.comment ?? '' }}</td></tr></tbody></table><div class="subsection-heading indexes-heading"><span><Link2 :size="13" /> {{ t('dbIndexes') }}</span><small>{{ currentIndexes.length }}</small></div><table class="structure-table"><thead><tr><th>{{ t('dbIndexName') }}</th><th>{{ t('dbIndexColumns') }}</th><th>{{ t('dbColumnType') }}</th><th>{{ t('dbUnique') }}</th></tr></thead><tbody><tr v-for="index in currentIndexes" :key="index.name"><td class="mono">{{ index.name }}</td><td>{{ index.columns.join(', ') }}</td><td>{{ index.type }}</td><td>{{ index.unique ? t('dbYes') : t('dbNo') }}</td></tr></tbody></table></div><div v-else class="result-empty compact"><strong>{{ t('dbStructureEmpty') }}</strong></div></div>
          <div v-else-if="resultView === 'ddl'" class="ddl-view"><div class="ddl-heading"><span>{{ t('dbShowCreateTable', { table: activeTable ?? '' }) }}</span><button class="ghost-button" @click="copyText(ddlByTable[`${activeConnectionId}:${activeTable}`] ?? '', t('dbToastDdlCopied'))"><Copy :size="13" /> {{ t('dbCopyDdl') }}</button></div><pre>{{ ddlByTable[`${activeConnectionId}:${activeTable}`] ?? t('dbLoadingDdl') }}</pre></div>
          <div v-else class="history-view"><div class="history-toolbar"><label class="search-field"><Search :size="13" /><input v-model="historySearch" :placeholder="t('dbSearchHistory')" /></label><button class="ghost-button" :disabled="!history.length" @click="clearHistory"><Trash2 :size="13" /> {{ t('dbClear') }}</button></div><div v-if="filteredHistory.length" class="history-list"><div v-for="entry in filteredHistory" :key="entry.id" class="history-row"><div class="history-status" :class="entry.success ? 'success' : 'error'"><Check v-if="entry.success" :size="12" /><CircleAlert v-else :size="12" /></div><div class="history-sql"><code>{{ entry.sqlText }}</code><small>{{ connections.find((item) => item.id === entry.connectionId)?.name ?? t('dbUnknownConnection') }} · {{ entry.database ?? t('dbNoDatabase') }} · {{ entry.executionTime }} ms</small></div><button class="ghost-button" @click="runHistoryEntry(entry)"><Play :size="12" /> {{ t('dbReRun') }}</button></div></div><div v-else class="result-empty compact"><History :size="17" /><strong>{{ t('dbHistoryEmptyTitle') }}</strong><p>{{ t('dbHistoryEmptyHint') }}</p></div></div>
        </section>
      </main>
    </div>
    <Transition name="toast"><div v-if="toast" class="db-toast"><Check :size="14" />{{ toast }}</div></Transition>

    <div v-if="pendingWarnings" class="modal-backdrop" @mousedown.self="pendingWarnings = undefined"><section class="danger-dialog" role="dialog" aria-modal="true"><div class="danger-icon"><ShieldAlert :size="20" /></div><div><span class="section-kicker danger-kicker">{{ t('dbWarningsKicker') }}</span><h2>{{ t('dbWarningsTitle') }}</h2><p>{{ t('dbWarningsHint') }}</p></div><div class="warning-list"><div v-for="warning in pendingWarnings.warnings" :key="warning.kind"><strong>{{ translateSqlWarning(warning.kind).title }}</strong><span>{{ translateSqlWarning(warning.kind).detail }}</span></div></div><pre>{{ pendingWarnings.sql }}</pre><div class="dialog-actions"><button class="ghost-button" @click="pendingWarnings = undefined">{{ t('cancel') }}</button><button class="danger-button" @click="confirmPendingQuery">{{ t('dbExecuteAnyway') }}</button></div></section></div>
  <div v-if="showConnectionDialog" class="modal-backdrop" @mousedown.self="closeDialog"><section class="connection-dialog" role="dialog" aria-modal="true" aria-labelledby="connection-dialog-title"><header><div><span class="section-kicker">{{ t('dbConnectionProfile') }}</span><h2 id="connection-dialog-title">{{ t('dbNewConnectionTitle') }}</h2></div><button class="icon-button" :aria-label="t('close')" @click="closeDialog"><X :size="16" /></button></header><div class="connection-form"><label class="wide-field">{{ t('dbConnectionName') }}<input v-model="draft.name" :placeholder="t('dbConnectionNamePlaceholder')" autofocus /></label><label>{{ t('dbDatabaseType') }}<select v-model="draft.type"><option value="mysql">MySQL</option><option value="sqlite">SQLite</option><option value="postgresql">{{ t('dbPostgresPreview') }}</option></select></label><label v-if="draft.type !== 'sqlite'">{{ t('dbHost') }}<input v-model="draft.host" placeholder="localhost" /></label><label v-if="draft.type !== 'sqlite'">{{ t('port') }}<input v-model.number="draft.port" type="number" placeholder="3306" /></label><label v-if="draft.type !== 'sqlite'">{{ t('dbUsername') }}<input v-model="draft.username" placeholder="root" /></label><label v-if="draft.type !== 'sqlite'">{{ t('dbPassword') }}<input v-model="draft.password" type="password" :placeholder="t('dbPasswordPlaceholder')" /></label><label v-if="draft.type !== 'sqlite'" class="wide-field">{{ t('database') }} <span class="optional">{{ t('optional') }}</span><input v-model="draft.database" placeholder="mcp_conductor_dev" /></label><label v-else class="wide-field">{{ t('dbSqlitePath') }}<input v-model="draft.sqlitePath" placeholder="./data/local.sqlite" /></label><label class="wide-field form-note"><input v-model="rememberPassword" type="checkbox" /> {{ t('dbRememberPassword') }} <span>{{ t('dbRememberPasswordHint') }}</span></label><div v-if="testResult" class="test-result" :class="{ success: testResult.success, failure: !testResult.success }"><Check v-if="testResult.success" :size="14" /><CircleAlert v-else :size="14" /><span>{{ translateDatabaseMessage(testResult.message) }}<small v-if="testResult.success">{{ testResult.serverVersion }} · {{ testResult.latencyMs }} ms</small><small v-else>{{ testResult.errorCode }}</small></span></div></div><footer><button class="ghost-button" @click="closeDialog">{{ t('cancel') }}</button><button class="ghost-button" :disabled="testingConnection" @click="testDraft"><LoaderCircle v-if="testingConnection" class="spin" :size="13" /><Zap v-else :size="13" /> {{ t('dbTestConnection') }}</button><button class="primary" @click="saveDraft"><Check :size="13" /> {{ t('dbSaveAndConnect') }}</button></footer></section></div>
  </section>
</template>

<style scoped>
.database-view { min-width: 0; height: 100%; padding: 24px 28px 30px; overflow: hidden; }
.database-header { min-height: 66px; margin-bottom: 18px; align-items: center; }
.database-header h1 { margin-top: 3px; }
.database-header-actions { display: flex; align-items: center; gap: 12px; }
.local-badge { display: inline-flex; align-items: center; gap: 7px; padding: 5px 8px; border: 1px solid rgb(81 195 148 / .22); border-radius: 999px; color: var(--success); font-size: 10px; font-weight: 700; }
.local-badge span { width: 6px; height: 6px; border-radius: 50%; background: var(--success); box-shadow: 0 0 0 3px var(--success-soft); }
.transaction-banner { display: flex; align-items: center; gap: 8px; min-height: 34px; margin-bottom: 10px; padding: 0 11px; border-left: 2px solid var(--warning); background: var(--warning-soft); color: var(--warning); font-size: 10px; }
.transaction-banner span { color: var(--muted); }.transaction-banner button { margin-left: auto; padding: 3px 7px; border: 0; background: transparent; color: var(--warning); font-size: 10px; font-weight: 700; }.transaction-banner button + button { margin-left: 0; }
.database-layout { display: grid; grid-template-columns: 255px minmax(0, 1fr); min-height: 0; height: calc(100% - 86px); border: 1px solid var(--border); background: var(--surface); box-shadow: var(--shadow-sm); }
.database-explorer { display: flex; min-width: 0; min-height: 0; flex-direction: column; border-right: 1px solid var(--border); background: var(--sidebar); }
.explorer-heading { display: flex; align-items: center; justify-content: space-between; min-height: 43px; padding: 0 10px 0 13px; border-bottom: 1px solid var(--border); color: var(--foreground); font-size: 11px; font-weight: 700; }.explorer-heading span { display: inline-flex; align-items: center; gap: 7px; }
.connection-list { min-height: 0; overflow: auto; padding: 7px 6px; }.connection-row { display: grid; grid-template-columns: 17px 22px minmax(0, 1fr) 8px 25px; align-items: center; gap: 4px; min-height: 40px; padding: 4px 4px 4px 1px; border-left: 2px solid transparent; cursor: pointer; }.connection-row:hover { background: var(--surface-2); }.connection-row.active { border-left-color: var(--accent); background: var(--accent-soft); }.tree-toggle { display: grid; width: 17px; height: 22px; place-items: center; border: 0; background: transparent; color: var(--faint); }.db-type-icon { display: grid; width: 22px; height: 22px; place-items: center; border-radius: 5px; background: var(--surface-2); color: var(--accent); }.db-type-icon.sqlite { color: var(--success); }.connection-copy { min-width: 0; }.connection-copy strong, .connection-copy small { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }.connection-copy strong { color: var(--foreground); font-size: 11px; }.connection-copy small { margin-top: 2px; color: var(--faint); font: 9px "Cascadia Code", Consolas, monospace; }.connection-status { width: 6px; height: 6px; border-radius: 50%; background: var(--faint); }.connection-status.is-connected { background: var(--success); box-shadow: 0 0 0 3px var(--success-soft); }.connection-status.is-failed { background: var(--danger); }.connection-status.is-connecting { background: var(--warning); }.row-more { opacity: 0; }.connection-row:hover .row-more, .connection-row:focus-within .row-more { opacity: 1; }
.tree-children { margin-left: 18px; padding: 2px 0 5px 7px; border-left: 1px solid var(--border); }.tree-children.nested { margin-left: 17px; }.tree-row { display: flex; width: 100%; align-items: center; gap: 6px; min-height: 26px; padding: 3px 5px; border: 0; background: transparent; color: var(--muted); font-size: 10px; text-align: left; cursor: pointer; }.tree-row:hover, .tree-row.active { background: var(--surface-2); color: var(--foreground); }.tree-row.active { color: var(--accent); }.tree-row span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }.tree-row small { margin-left: auto; color: var(--faint); font: 9px "Cascadia Code", Consolas, monospace; }.collection-row { color: var(--faint); font-size: 9px; font-weight: 700; }.table-row { padding-left: 2px; }.view-row { color: var(--muted); }.table-subtree { margin-left: 24px; border-left: 1px solid var(--border); }.sub-row { min-height: 23px; color: var(--faint); font-size: 9px; }.tree-hint { display: flex; align-items: center; gap: 5px; padding: 7px 4px; color: var(--warning); font-size: 9px; line-height: 1.35; }.explorer-footer { display: flex; gap: 2px; margin-top: auto; padding: 7px; border-top: 1px solid var(--border); }.explorer-footer .ghost-button { flex: 1; justify-content: center; padding-inline: 5px; font-size: 9px; }.explorer-footer .ghost-button span { color: var(--accent); }.explorer-empty { display: grid; justify-items: center; gap: 8px; padding: 54px 20px 25px; text-align: center; }.explorer-empty strong { font-size: 11px; }.explorer-empty p { margin: 0; color: var(--muted); font-size: 9px; line-height: 1.5; }.empty-mark { display: grid; width: 34px; height: 34px; place-items: center; border: 1px solid var(--border-strong); border-radius: 7px; background: var(--surface-2); color: var(--accent); }
.database-main { display: grid; grid-template-rows: 43px 34px minmax(240px, 1fr) minmax(220px, .9fr); min-width: 0; min-height: 0; background: var(--background); }.database-toolbar { display: flex; align-items: center; justify-content: space-between; gap: 12px; padding: 0 12px; border-bottom: 1px solid var(--border); background: var(--surface); }.context-selects, .toolbar-actions, .editor-toolbar-left, .editor-toolbar-right, .result-actions, .result-tabs { display: flex; align-items: center; gap: 7px; }.context-selects label { display: inline-flex; align-items: center; gap: 6px; color: var(--faint); font-size: 9px; font-weight: 700; }.context-selects select { max-width: 150px; padding: 4px 20px 4px 6px; border: 1px solid var(--border); background: var(--surface-2); color: var(--foreground); font-size: 10px; }.slash { color: var(--border-strong); }.toolbar-actions .ghost-button { min-height: 26px; padding-inline: 7px; font-size: 9px; }
.query-tabs { display: flex; align-items: stretch; min-width: 0; overflow: auto hidden; border-bottom: 1px solid var(--border); background: var(--surface-2); }.query-tabs button { display: inline-flex; align-items: center; gap: 6px; min-width: 95px; padding: 0 10px; border: 0; border-right: 1px solid var(--border); border-bottom: 2px solid transparent; background: transparent; color: var(--muted); font: 10px "Cascadia Code", Consolas, monospace; white-space: nowrap; }.query-tabs button:hover { color: var(--foreground); }.query-tabs button.active { border-bottom-color: var(--accent); background: var(--surface); color: var(--foreground); }.query-tabs button span { color: var(--accent); }.query-tabs .tab-close { margin-left: auto; color: var(--faint); }.query-tabs .new-tab { min-width: 34px; color: var(--faint); }
.sql-editor-panel { display: flex; min-height: 0; flex-direction: column; border-bottom: 1px solid var(--border); background: #101219; }.editor-toolbar { display: flex; align-items: center; justify-content: space-between; min-height: 38px; padding: 0 10px; border-bottom: 1px solid #282c35; background: #171a21; color: #a9afbc; }.dialect-chip { padding: 3px 6px; border: 1px solid rgb(147 136 245 / .35); border-radius: 4px; color: #aaa1ff; font: 9px "Cascadia Code", Consolas, monospace; }.editor-state { display: inline-flex; align-items: center; gap: 5px; font-size: 9px; }.editor-state .status-dot { width: 5px; height: 5px; background: #626978; }.editor-state .status-dot.is-active { background: #e4b75e; }.compact-control { display: inline-flex; align-items: center; gap: 5px; color: #7f8694; font-size: 9px; }.compact-control input, .compact-control select { width: 60px; padding: 3px 4px; border: 1px solid #313640; background: #20242d; color: #cdd2dd; font: 9px "Cascadia Code", Consolas, monospace; }.compact-control select { width: 67px; }.editor-toolbar .icon-button, .editor-toolbar .ghost-button { color: #a9afbc; }.editor-toolbar .primary { min-height: 25px; padding-inline: 8px; background: #7b70df; font-size: 10px; }.run-button kbd { margin-left: 6px; color: #d5d1ff; font-size: 8px; }.danger-outline { display: inline-flex; align-items: center; gap: 5px; min-height: 25px; padding: 0 8px; border: 1px solid #98525b; background: transparent; color: #ef9399; font-size: 10px; }.editor-wrap { display: grid; grid-template-columns: 37px minmax(0, 1fr); min-height: 0; flex: 1; }.line-numbers { padding: 12px 9px 12px 0; border-right: 1px solid #222630; color: #505764; font: 11px/1.75 "Cascadia Code", Consolas, monospace; text-align: right; user-select: none; }.sql-editor { width: 100%; height: 100%; min-height: 150px; resize: none; padding: 12px 16px; border: 0; outline: 0; background: transparent; color: #dfe3ec; caret-color: #b4abff; font: 12px/1.75 "Cascadia Code", Consolas, monospace; tab-size: 2; }.sql-editor::selection { background: rgb(123 112 223 / .34); }.editor-footer { display: flex; align-items: center; justify-content: space-between; gap: 12px; min-height: 31px; padding: 0 10px 0 47px; border-top: 1px solid #282c35; color: #666e7c; font-size: 9px; }.key-hint { color: #9690e8; font-family: "Cascadia Code", Consolas, monospace; }.transaction-actions { display: flex; gap: 2px; }.transaction-actions .ghost-button { min-height: 22px; padding-inline: 6px; color: #8c93a0; font-size: 9px; }
.result-panel { display: flex; min-height: 0; flex-direction: column; background: var(--surface); }.result-header { display: flex; align-items: center; justify-content: space-between; min-height: 39px; padding: 0 9px; border-bottom: 1px solid var(--border); }.result-tabs { height: 100%; gap: 2px; }.result-tabs button { align-self: stretch; padding: 0 9px; border: 0; border-bottom: 2px solid transparent; background: transparent; color: var(--muted); font-size: 10px; }.result-tabs button:hover { color: var(--foreground); }.result-tabs button.active { border-bottom-color: var(--accent); color: var(--foreground); }.result-tabs button span { margin-left: 3px; color: var(--accent); font: 9px "Cascadia Code", Consolas, monospace; }.result-actions .icon-button { min-width: 25px; height: 25px; }.result-content, .messages-view, .explain-view, .structure-view, .ddl-view, .history-view { min-height: 0; flex: 1; overflow: auto; }.result-empty { display: grid; min-height: 100%; place-items: center; align-content: center; gap: 8px; color: var(--muted); text-align: center; }.result-empty strong { color: var(--foreground); font-size: 11px; }.result-empty p { margin: 0; color: var(--faint); font-size: 9px; }.result-empty kbd { padding: 2px 4px; border: 1px solid var(--border); border-radius: 3px; color: var(--accent); font: 9px "Cascadia Code", Consolas, monospace; }.result-empty.compact { min-height: 160px; }.grid-frame { display: flex; height: 100%; min-height: 0; flex-direction: column; }.grid-scroll { min-height: 0; flex: 1; overflow: auto; overscroll-behavior: contain; }.data-grid { width: max-content; min-width: 100%; border-collapse: collapse; font-size: 10px; }.data-grid th, .data-grid td { max-width: 480px; padding: 7px 10px; border-bottom: 1px solid var(--border); text-align: left; white-space: nowrap; }.data-grid th { position: sticky; top: 0; z-index: 1; background: var(--surface-2); color: var(--muted); font-weight: 700; }.data-grid th small { display: block; margin-top: 2px; color: var(--faint); font: 8px "Cascadia Code", Consolas, monospace; font-weight: 400; }.data-grid td { overflow: hidden; text-overflow: ellipsis; }.data-grid.is-wrapped th, .data-grid.is-wrapped td { white-space: pre-wrap; overflow-wrap: anywhere; vertical-align: top; }.data-grid.is-wrapped td { overflow: visible; text-overflow: clip; }.data-grid tbody tr:hover { background: var(--surface-2); }.row-number { width: 38px; color: var(--faint) !important; font: 9px "Cascadia Code", Consolas, monospace; text-align: right !important; }.grid-spacer { width: 100%; }.row-actions { width: 34px; padding-inline: 4px !important; }.row-actions .icon-button { opacity: 0; }.data-grid tr:hover .row-actions .icon-button { opacity: 1; }.value-null { color: var(--faint); font-style: italic; }.value-number { color: #74c6ec; font-family: "Cascadia Code", Consolas, monospace; }.value-boolean { color: #a9a0ff; font-family: "Cascadia Code", Consolas, monospace; }.value-date { color: #dfa966; font-family: "Cascadia Code", Consolas, monospace; }.value-json { color: #74c99d; font-family: "Cascadia Code", Consolas, monospace; }.grid-footer { display: flex; align-items: center; gap: 12px; min-height: 31px; padding: 0 10px; border-top: 1px solid var(--border); color: var(--faint); font: 9px "Cascadia Code", Consolas, monospace; }.grid-footer .ghost-button { margin-left: auto; min-height: 22px; padding-inline: 6px; font-size: 9px; }.grid-footer .ghost-button.active { border-color: var(--accent); background: var(--accent-soft); color: var(--accent); }.truncated-note { display: inline-flex; align-items: center; gap: 5px; color: var(--warning); }
.message-row { display: flex; align-items: flex-start; gap: 8px; padding: 11px 13px; border-bottom: 1px solid var(--border); color: var(--success); font: 10px "Cascadia Code", Consolas, monospace; }.message-row.error { color: var(--danger); }.message-row span { white-space: pre-wrap; }.explain-insights { display: flex; gap: 18px; padding: 9px 12px; color: var(--success); font-size: 9px; }.explain-insights span { display: inline-flex; align-items: center; gap: 5px; }.structure-view { padding: 12px; }.subsection-heading { display: flex; align-items: center; justify-content: space-between; margin: 0 0 7px; color: var(--foreground); font-size: 10px; font-weight: 700; }.subsection-heading span { display: inline-flex; align-items: center; gap: 6px; }.subsection-heading small { color: var(--accent); font: 9px "Cascadia Code", Consolas, monospace; }.indexes-heading { margin-top: 21px; }.structure-table { width: 100%; border-collapse: collapse; font-size: 9px; }.structure-table th, .structure-table td { padding: 7px 8px; border-bottom: 1px solid var(--border); text-align: left; }.structure-table th { color: var(--muted); font-size: 9px; font-weight: 600; }.structure-table td { color: var(--foreground); }.structure-table .key-icon { display: inline-block; margin-right: 5px; color: var(--warning); vertical-align: -2px; }.type-cell { color: var(--accent) !important; }.muted-cell { color: var(--muted) !important; }.ddl-view { padding: 12px; }.ddl-heading { display: flex; align-items: center; justify-content: space-between; color: var(--muted); font: 9px "Cascadia Code", Consolas, monospace; }.ddl-heading .ghost-button { font-family: var(--font-family, inherit); }.ddl-view pre { min-height: 130px; margin: 9px 0 0; padding: 12px; overflow: auto; border: 1px solid var(--border); background: #101219; color: #d7dbe6; font: 11px/1.7 "Cascadia Code", Consolas, monospace; white-space: pre-wrap; }.history-toolbar { display: flex; align-items: center; justify-content: space-between; gap: 10px; padding: 9px 11px; border-bottom: 1px solid var(--border); }.history-toolbar .search-field { flex: 1; max-width: 330px; }.history-toolbar .search-field input { width: 100%; }.history-row { display: grid; grid-template-columns: 25px minmax(0, 1fr) auto; align-items: center; gap: 9px; padding: 9px 12px; border-bottom: 1px solid var(--border); }.history-row:hover { background: var(--surface-2); }.history-status { display: grid; width: 20px; height: 20px; place-items: center; border-radius: 5px; background: var(--success-soft); color: var(--success); }.history-status.error { background: var(--danger-soft); color: var(--danger); }.history-sql { min-width: 0; }.history-sql code { display: block; overflow: hidden; color: var(--foreground); font: 10px "Cascadia Code", Consolas, monospace; text-overflow: ellipsis; white-space: nowrap; }.history-sql small { display: block; margin-top: 4px; color: var(--faint); font-size: 9px; }.history-row .ghost-button { min-height: 24px; padding-inline: 7px; font-size: 9px; }
.modal-backdrop { position: fixed; inset: 0; z-index: 60; display: grid; place-items: center; padding: 24px; background: rgb(5 6 10 / .68); }.connection-dialog, .danger-dialog { width: min(520px, 100%); border: 1px solid var(--border-strong); background: var(--surface); box-shadow: 0 24px 80px rgb(0 0 0 / .4); }.connection-dialog header { display: flex; align-items: flex-start; justify-content: space-between; padding: 17px 18px; border-bottom: 1px solid var(--border); }.connection-dialog h2, .danger-dialog h2 { margin: 3px 0 0; font-size: 15px; }.connection-form { display: grid; grid-template-columns: 1fr 1fr; gap: 12px; padding: 17px 18px; }.connection-form label { display: grid; gap: 5px; color: var(--muted); font-size: 10px; font-weight: 600; }.connection-form input:not([type='checkbox']), .connection-form select { width: 100%; padding: 8px 9px; border: 1px solid var(--border); background: var(--surface-2); color: var(--foreground); font-size: 11px; }.connection-form input:focus, .connection-form select:focus { border-color: var(--accent); outline: 2px solid var(--accent-soft); }.wide-field { grid-column: 1 / 3; }.optional { color: var(--faint); font-size: 9px; font-weight: 400; }.form-note { display: flex !important; grid-template-columns: auto 1fr; grid-template-rows: auto auto; align-items: center; gap: 5px 7px !important; padding-top: 3px; font-weight: 500 !important; }.form-note input { grid-row: 1 / 3; accent-color: var(--accent); }.form-note span { color: var(--faint); font-size: 9px; font-weight: 400; }.test-result { display: flex; grid-column: 1 / 3; align-items: flex-start; gap: 8px; padding: 9px 10px; font-size: 10px; }.test-result.success { background: var(--success-soft); color: var(--success); }.test-result.failure { background: var(--danger-soft); color: var(--danger); }.test-result span { display: grid; gap: 3px; }.test-result small { color: var(--muted); font: 9px "Cascadia Code", Consolas, monospace; }.connection-dialog footer, .dialog-actions { display: flex; justify-content: flex-end; gap: 7px; padding: 12px 18px; border-top: 1px solid var(--border); }.connection-dialog footer .primary { min-height: 30px; }.danger-dialog { display: grid; grid-template-columns: auto 1fr; gap: 0 12px; padding: 19px; }.danger-icon { display: grid; width: 36px; height: 36px; place-items: center; border: 1px solid var(--danger); background: var(--danger-soft); color: var(--danger); }.danger-kicker { color: var(--danger); }.danger-dialog p { grid-column: 1 / 3; margin: 14px 0 0; color: var(--muted); font-size: 10px; line-height: 1.5; }.warning-list { display: grid; grid-column: 1 / 3; gap: 7px; margin-top: 14px; }.warning-list div { display: grid; gap: 3px; padding: 8px 9px; border-left: 2px solid var(--warning); background: var(--warning-soft); }.warning-list strong { color: var(--foreground); font-size: 10px; }.warning-list span { color: var(--muted); font-size: 9px; }.danger-dialog pre { grid-column: 1 / 3; max-height: 100px; margin: 13px 0 0; padding: 10px; overflow: auto; background: #101219; color: #e6e8ef; font: 10px/1.6 "Cascadia Code", Consolas, monospace; white-space: pre-wrap; }.dialog-actions { grid-column: 1 / 3; margin: 18px -19px -19px; }.dialog-actions .danger-button { color: var(--danger); }.db-toast { position: fixed; right: 24px; bottom: 42px; z-index: 70; display: inline-flex; align-items: center; gap: 7px; padding: 9px 12px; border: 1px solid var(--border-strong); background: var(--surface); box-shadow: 0 12px 30px rgb(0 0 0 / .22); color: var(--foreground); font-size: 10px; }.toast-enter-active, .toast-leave-active { transition: opacity .18s, transform .18s; }.toast-enter-from, .toast-leave-to { opacity: 0; transform: translateY(5px); }.spin { animation: spin .8s linear infinite; }@keyframes spin { to { transform: rotate(360deg); } }
@media (max-width: 980px) { .database-layout { grid-template-columns: 215px minmax(0, 1fr); }.database-header-actions .local-badge { display: none; }.database-view { padding-inline: 18px; } }
@media (max-width: 760px) { .database-view { min-width: 700px; }.database-main { grid-template-rows: 43px 34px minmax(230px, 1fr) minmax(200px, .9fr); }.editor-toolbar-right .compact-control, .editor-footer > span { display: none; }.editor-footer { padding-left: 10px; }.database-header { align-items: flex-start; flex-direction: column; gap: 12px; }.database-header-actions { width: 100%; justify-content: flex-end; } }
</style>
